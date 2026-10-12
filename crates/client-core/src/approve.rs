//! Answering a Claude permission prompt from the list (tmls `approve.py`): the dialog is read
//! off the session's pane, shown, and answered with `1` (Yes) or Escape (No) — only if a fresh
//! capture still shows the same dialog. Pure text here; the commands are `tmux::capture_argv`
//! and `tmux::send_keys_argv`.

/// The line Claude's permission dialog asks.
pub const QUESTION: &str = "Do you want to proceed?";
/// Approves at once.
pub const YES_KEY: &str = "1";
/// Declines: the number for No differs from prompt to prompt, Escape does not.
pub const NO_KEY: &str = "Escape";
/// The most lines of a dialog the form shows.
pub const MAX_LINES: usize = 12;

/// The dialog's lines (tool, description, command) when a permission prompt is up in `screen`:
/// from the last rule (`──…`) above the last `QUESTION` down to it, trimmed, without blank
/// lines, dashed rules (`╌`) and tips. `None` when nothing asks.
pub fn request(screen: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = screen.lines().collect();
    let ask = lines.iter().rposition(|line| line.trim() == QUESTION)?;
    let top = lines[..ask]
        .iter()
        .rposition(|line| line.starts_with("──"))
        .map_or(0, |rule| rule + 1);
    let dialog = lines[top..ask]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('╌') && !line.starts_with("Tip:"))
        .map(str::to_string)
        .collect();
    Some(dialog)
}

/// Whether two captures show the same dialog.
pub fn same(a: &[String], b: &[String]) -> bool {
    a == b
}

/// The dialog as the form shows it: at most `MAX_LINES`, the middle replaced by one line that
/// says how many are left out.
pub fn shown(dialog: &[String]) -> Vec<String> {
    if dialog.len() <= MAX_LINES {
        return dialog.to_vec();
    }
    let head = MAX_LINES / 2;
    let tail = MAX_LINES - head - 1;
    let mut out = dialog[..head].to_vec();
    out.push(format!("… {} more lines …", dialog.len() - head - tail));
    out.extend_from_slice(&dialog[dialog.len() - tail..]);
    out
}
