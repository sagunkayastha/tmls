//! Links in the terminal: the URL or the file path under a cell, and opening one.
//!
//! As tmls (`term.py`, `link_at`): a URL is `http://` or `https://` up to the next white space
//! or one of `< > " ' \``, without the punctuation a sentence puts after it (a closing bracket
//! stays when the URL has its opening one). The text is taken as it is on screen: `localhost`
//! stays `localhost`, also in a session on another host. A cell that carries a hyperlink of its
//! own (OSC 8) gives that one. Only one row is looked at, so a link that wraps is cut at the
//! edge (tmls joins the next row).
//!
//! **File paths** (`link_at`), in tmls's order — the first kind that covers the cell wins, and
//! a URL always does:
//! - `PATH:LINE` and `PATH:LINE:COL` (tmls `FILE_LINE`): a name with an extension
//!   (`main.rs:12`) or anything with a `/` in it (`src/x:3`, `/etc/hosts:1`), of the
//!   characters `A-Z a-z 0-9 . _ ~ / + -`;
//! - an absolute or home path with no line (tmls `BARE_PATH`: `/data/x.png`, `~/notes/a.md`),
//!   not in the middle of a word (`and/or`, `1/2`), without the full stop after it;
//! - any other path tmls's quick select would offer (`quickselect.py`, `PATH`): `./x`,
//!   `../x`, `src/main.rs`, `Cargo.toml`. tmls takes these from the keyboard only; here a
//!   Ctrl+click on one opens it too.
//!
//! A path is only text: whether there is such a file is the host's to say (`viewer`).
//!
//! Opening runs `xdg-open TARGET` with the target as one argument, without a shell: a URL only
//! if it is `http://` or `https://` (`open`), a file only by its absolute path (`open_path`).

use std::io;
use std::process::{Command, Stdio};

use s2_core::{Frame, Wide};

const SCHEMES: [&str; 2] = ["http://", "https://"];
const ENDS: &str = "<>\"'`";
const TRAILING: &str = ".,;:!?)]}'\"";

/// What a Ctrl+click on a cell opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// For the browser.
    Url(String),
    File(FileLink),
}

/// A file path as the terminal shows it, not yet looked for on any host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    /// Absolute, under `~/`, or relative to the directory of the session's pane.
    pub path: String,
    /// The `:LINE` after it, counted from 1.
    pub line: Option<usize>,
}

/// Whether `url` is one this app opens.
pub fn allowed(url: &str) -> bool {
    SCHEMES
        .iter()
        .any(|scheme| url.len() > scheme.len() && url.starts_with(scheme))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The URLs of a row: where each starts, where it ends, and where it ends with everything up
/// to the next white space (the punctuation after it included).
fn urls(chars: &[char]) -> Vec<(usize, usize, usize)> {
    let starts_at = |i: usize, scheme: &str| {
        scheme
            .chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(scheme) = SCHEMES.iter().find(|scheme| starts_at(i, scheme)) else {
            i += 1;
            continue;
        };
        let mut end = i + scheme.len();
        while end < chars.len() && !chars[end].is_whitespace() && !ENDS.contains(chars[end]) {
            end += 1;
        }
        let whole = end;
        while end > i && TRAILING.contains(chars[end - 1]) {
            let count = |c: char| chars[i..end].iter().filter(|&&x| x == c).count();
            if chars[end - 1] == ')' && count('(') >= count(')') {
                break;
            }
            end -= 1;
        }
        if end > i + scheme.len() {
            found.push((i, end, whole));
        }
        i = whole.max(i + 1);
    }
    found
}

/// The URL that covers column `col` of `row_text` (columns count characters), if any.
pub fn at(row_text: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = row_text.chars().collect();
    urls(&chars)
        .into_iter()
        .find(|(start, end, _)| (*start..*end).contains(&col))
        .map(|(start, end, _)| chars[start..end].iter().collect())
}

/// A character of a path (tmls: `[A-Za-z0-9._~/+-]`).
fn path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._~/+-".contains(c)
}

/// A character of one name of a path: `path_char` without the `/`.
fn name_char(c: char) -> bool {
    c != '/' && path_char(c)
}

/// Where the run of characters `is` accepts ends that starts at `from`.
fn run_end(chars: &[char], from: usize, is: impl Fn(char) -> bool) -> usize {
    let run = chars[from.min(chars.len())..]
        .iter()
        .take_while(|c| is(**c));
    from + run.count()
}

/// Python's `\w`: a letter, a digit or `_`, in any script.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The digits from `from`: where they end and their number (the largest there is when they
/// are too many).
fn number(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let end = run_end(chars, from, |c| c.is_ascii_digit());
    let digits: String = chars[from.min(end)..end].iter().collect();
    (end > from).then(|| (end, digits.parse().unwrap_or(usize::MAX)))
}

/// tmls's `FILE_LINE` at `i`: `(end of the path, end of the match, line)`.
///
/// ```text
/// (?P<path>[A-Za-z0-9._~/+-]*[A-Za-z0-9_~-]\.[A-Za-z0-9]+|[A-Za-z0-9._~+-]*/[A-Za-z0-9._~/+-]+)
/// :(?P<line>\d+)(?::\d+)?
/// ```
///
/// Both forms of the path are runs of `path_char` that end where the `:` is, so the path is
/// the whole run from `i`; what differs is what the run must hold: a name, a dot and an
/// extension of letters and digits at its end, or a `/` with something after it.
fn file_line(chars: &[char], i: usize) -> Option<(usize, usize, usize)> {
    let end = run_end(chars, i, path_char);
    if end == i || chars.get(end) != Some(&':') {
        return None;
    }
    let (line_end, line) = number(chars, end + 1)?;
    let run = &chars[i..end];
    // `name.ext`: the extension is what follows the last character that is no letter or digit.
    let ext = run
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let dot = run.len().checked_sub(ext + 1);
    let named = ext > 0
        && dot.is_some_and(|dot| {
            let before = |c: &char| c.is_ascii_alphanumeric() || "_~-".contains(*c);
            run[dot] == '.' && dot > 0 && before(&run[dot - 1])
        });
    // `…/…`: the first `/` is not the run's last character.
    let slashed = run
        .iter()
        .position(|c| *c == '/')
        .is_some_and(|slash| slash + 1 < run.len());
    if !(named || slashed) {
        return None;
    }
    // `:COL` belongs to the link and is not kept.
    let col_end = match chars.get(line_end) {
        Some(':') => number(chars, line_end + 1).map(|(end, _)| end),
        _ => None,
    };
    Some((end, col_end.unwrap_or(line_end), line))
}

/// tmls's `BARE_PATH` at `i`: where the path ends, with the punctuation after it.
///
/// ```text
/// (?<![\w.~/-])(?:~/|/)[A-Za-z0-9._~+-][A-Za-z0-9._~+/-]*
/// ```
fn bare_path(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (word_char(chars[i - 1]) || ".~/-".contains(chars[i - 1])) {
        return None;
    }
    let first = match (chars.get(i), chars.get(i + 1)) {
        (Some('~'), Some('/')) => i + 2,
        (Some('/'), _) => i + 1,
        _ => return None,
    };
    chars
        .get(first)
        .is_some_and(|c| name_char(*c))
        .then(|| run_end(chars, first, path_char))
}

/// tmls's quick-select `PATH` at `i`: where the path ends.
///
/// ```text
/// (?<![\w./])(?:~?/|\.{1,2}/)[A-Za-z0-9._/+~-]+
/// (?<![\w./])[A-Za-z0-9._+-]+(?:/[A-Za-z0-9._+-]+)+
/// (?<![\w./])[A-Za-z0-9_+-]+\.[A-Za-z0-9]+
/// ```
///
/// Two things differ from tmls, whose pattern is for hints one looks at before picking: a
/// name may have more than one dot (`x.tar.gz`; tmls stops at `x.tar`), and the full stop of
/// a sentence after a name is not the path's (`see src/main.rs.`).
fn any_path(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (word_char(chars[i - 1]) || "./".contains(chars[i - 1])) {
        return None;
    }
    let is = |at: usize, c: char| chars.get(at) == Some(&c);
    let without_stop = |end: usize| {
        let stop = end >= i + 2 && chars[end - 1] == '.' && chars[end - 2].is_ascii_alphanumeric();
        if stop { end - 1 } else { end }
    };
    // `/x`, `~/x`, `./x`, `../x`.
    let rooted = if is(i, '~') && is(i + 1, '/') {
        Some(i + 2)
    } else if is(i, '/') {
        Some(i + 1)
    } else if is(i, '.') && is(i + 1, '.') && is(i + 2, '/') {
        Some(i + 3)
    } else if is(i, '.') && is(i + 1, '/') {
        Some(i + 2)
    } else {
        None
    };
    if let Some(from) = rooted {
        let end = run_end(chars, from, path_char);
        if end > from {
            return Some(without_stop(end));
        }
    }
    // `a/b/c`: names with a `/` between them, two at least.
    let part = |c: char| c != '~' && name_char(c);
    let first = run_end(chars, i, part);
    let mut end = first;
    let mut names = usize::from(end > i);
    while names > 0 && is(end, '/') {
        let next = run_end(chars, end + 1, part);
        if next == end + 1 {
            break;
        }
        (end, names) = (next, names + 1);
    }
    if names >= 2 {
        return Some(without_stop(end));
    }
    // `name.ext`: a name that does not start with a dot and ends in one and letters or digits.
    let end = without_stop(first);
    let name = &chars[i..end];
    let ext = name
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let dotted = ext > 0 && ext + 1 < name.len() && name[name.len() - ext - 1] == '.';
    (dotted && name[0] != '.').then_some(end)
}

/// The links of a row, each with the columns it covers. A URL keeps a path from being found
/// in it; `PATH:LINE` comes before a bare path, and both before `any_path`.
fn links(chars: &[char]) -> Vec<(usize, usize, Link)> {
    let urls = urls(chars);
    let in_url =
        |start: usize, end: usize| urls.iter().any(|(from, _, to)| start < *to && end > *from);
    let text = |start: usize, end: usize| chars[start..end].iter().collect::<String>();
    let file = |path: String, line| Link::File(FileLink { path, line });
    let mut found: Vec<(usize, usize, Link)> = urls
        .iter()
        .map(|(start, end, _)| (*start, *end, Link::Url(text(*start, *end))))
        .collect();

    let mut i = 0;
    while i < chars.len() {
        let Some((path_end, end, line)) = file_line(chars, i) else {
            i += 1;
            continue;
        };
        if !in_url(i, end) {
            found.push((i, end, file(text(i, path_end), Some(line))));
        }
        i = end;
    }
    let free = |found: &[(usize, usize, Link)], start: usize, end: usize| {
        !found.iter().any(|(from, to, _)| start < *to && end > *from)
    };
    let mut i = 0;
    while i < chars.len() {
        let Some(whole) = bare_path(chars, i) else {
            i += 1;
            continue;
        };
        let mut end = whole;
        while end > i && TRAILING.contains(chars[end - 1]) {
            end -= 1;
        }
        // tmls looks at the kinds in turn for one cell, so the first that covers it wins.
        if end > i + 1 && !in_url(i, whole) && free(&found, i, end) {
            found.push((i, end, file(text(i, end), None)));
        }
        i = whole;
    }
    let mut i = 0;
    while i < chars.len() {
        let Some(end) = any_path(chars, i) else {
            i += 1;
            continue;
        };
        if free(&found, i, end) {
            found.push((i, end, file(text(i, end), None)));
        }
        i = end;
    }
    found
}

/// The link that covers column `col` of `row_text` (columns count characters), if any.
pub fn link_at(row_text: &str, col: usize) -> Option<Link> {
    let chars: Vec<char> = row_text.chars().collect();
    links(&chars)
        .into_iter()
        .find(|(start, end, _)| (*start..*end).contains(&col))
        .map(|(_, _, link)| link)
}

/// The text of row `row` of `frame` and the hyperlink the cell at `col` carries. One
/// character per cell, so the column is the character index (the second cell of a wide
/// character counts as a space).
fn row_of(frame: &Frame, (col, row): (u16, u16)) -> Option<(String, Option<String>)> {
    let cols = frame.cols as usize;
    let start = row as usize * cols;
    let cells = frame.cells.get(start..start + cols)?;
    let own = cells
        .get(col as usize)
        .and_then(|cell| cell.hyperlink.clone());
    let text: String = cells
        .iter()
        .map(|cell| match cell.wide {
            Wide::Spacer => ' ',
            _ => cell.text.chars().next().unwrap_or(' '),
        })
        .collect();
    Some((text, own))
}

/// The URL under the cell (`col`, `row`) of `frame`: the cell's own hyperlink, else the URL in
/// the row's text.
pub fn in_frame(frame: &Frame, at_cell: (u16, u16)) -> Option<String> {
    let (text, own) = row_of(frame, at_cell)?;
    own.or_else(|| at(&text, at_cell.0 as usize))
}

/// The link under the cell (`col`, `row`) of `frame`: the cell's own hyperlink, else the URL or
/// the file path in the row's text.
pub fn link_in_frame(frame: &Frame, at_cell: (u16, u16)) -> Option<Link> {
    let (text, own) = row_of(frame, at_cell)?;
    own.map(Link::Url)
        .or_else(|| link_at(&text, at_cell.0 as usize))
}

/// Opens `url` with `spawn(program, arguments)` if it is `allowed`; anything else is refused
/// and nothing is run.
pub fn open_with(
    url: &str,
    spawn: &mut dyn FnMut(&str, &[&str]) -> io::Result<()>,
) -> Result<(), String> {
    if !allowed(url) {
        return Err(format!("not opened (only http and https links are): {url}"));
    }
    spawn("xdg-open", &[url]).map_err(|e| format!("xdg-open failed: {e}"))
}

/// Opens `url` in the user's browser: `xdg-open` in a process group of its own, with no
/// terminal and no shell.
pub fn open(url: &str) -> Result<(), String> {
    open_with(url, &mut detached)
}

/// Starts `program` in a process group of its own, with no terminal and no shell.
fn detached(program: &str, args: &[&str]) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    // Reaped when it ends, without waiting for it here.
    std::thread::spawn(move || drop(child.wait()));
    Ok(())
}

/// Opens the file or folder at `path` on this machine with `spawn(program, arguments)`. Only
/// an absolute path is taken: `xdg-open` has no `--`, and reads anything else as an option or
/// an address.
pub fn open_path_with(
    path: &str,
    spawn: &mut dyn FnMut(&str, &[&str]) -> io::Result<()>,
) -> Result<(), String> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(format!("not opened (not an absolute path): {path}"));
    }
    spawn("xdg-open", &[path]).map_err(|e| format!("xdg-open failed: {e}"))
}

/// Opens a folder with `spawn(program, arguments)`: an absolute path on this machine, or the
/// `sftp://` address of one on another host (`viewer::folder_target`). Anything else is
/// refused and nothing is run.
pub fn open_folder_with(
    target: &str,
    spawn: &mut dyn FnMut(&str, &[&str]) -> io::Result<()>,
) -> Result<(), String> {
    let remote = target
        .strip_prefix("sftp://")
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '-']));
    if !remote {
        return open_path_with(target, spawn);
    }
    if target.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("not opened (not an address): {target}"));
    }
    spawn("xdg-open", &[target]).map_err(|e| format!("xdg-open failed: {e}"))
}

/// Opens a folder in the desktop's file manager (`open_folder_with`).
pub fn open_folder(target: &str) -> Result<(), String> {
    open_folder_with(target, &mut detached)
}

/// Opens the file or folder at `path` in the desktop's app for it, as `open` does a URL.
pub fn open_path(path: &str) -> Result<(), String> {
    open_path_with(path, &mut detached)
}
