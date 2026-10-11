//! Links in the terminal: the URL under a cell, and opening one.
//!
//! As tmls (`term.py`, `link_at`): a URL is `http://` or `https://` up to the next white space
//! or one of `< > " ' \``, without the punctuation a sentence puts after it (a closing bracket
//! stays when the URL has its opening one). The text is taken as it is on screen: `localhost`
//! stays `localhost`, also in a session on another host. A cell that carries a hyperlink of its
//! own (OSC 8) gives that one. Only one row is looked at, so a URL that wraps is cut at the
//! edge.
//!
//! Opening runs `xdg-open URL` with the URL as one argument, without a shell, and only for
//! `http://` and `https://`.

use std::io;
use std::process::{Command, Stdio};

use s2_core::{Frame, Wide};

const SCHEMES: [&str; 2] = ["http://", "https://"];
const ENDS: &str = "<>\"'`";
const TRAILING: &str = ".,;:!?)]}'\"";

/// Whether `url` is one this app opens.
pub fn allowed(url: &str) -> bool {
    SCHEMES
        .iter()
        .any(|scheme| url.len() > scheme.len() && url.starts_with(scheme))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The URL that covers column `col` of `row_text` (columns count characters), if any.
pub fn at(row_text: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = row_text.chars().collect();
    let starts_at = |i: usize, scheme: &str| {
        scheme
            .chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
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
        if (i..end).contains(&col) && end > i + scheme.len() {
            return Some(chars[i..end].iter().collect());
        }
        i = whole.max(i + 1);
    }
    None
}

/// The link under the cell (`col`, `row`) of `frame`: the cell's own hyperlink, else the URL in
/// the row's text. One character per cell, so the column is the character index (the second
/// cell of a wide character counts as a space).
pub fn in_frame(frame: &Frame, (col, row): (u16, u16)) -> Option<String> {
    let cols = frame.cols as usize;
    let start = row as usize * cols;
    let cells = frame.cells.get(start..start + cols)?;
    if let Some(uri) = cells
        .get(col as usize)
        .and_then(|cell| cell.hyperlink.clone())
    {
        return Some(uri);
    }
    let text: String = cells
        .iter()
        .map(|cell| match cell.wide {
            Wide::Spacer => ' ',
            _ => cell.text.chars().next().unwrap_or(' '),
        })
        .collect();
    at(&text, col as usize)
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
    open_with(url, &mut |program, args| {
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
    })
}
