//! What runs where: the script that lists one host's tmux sessions, and the parser that turns
//! its output into a [`HostSnapshot`] with one status mark per session.
//!
//! Ported from tmls (`hosts.py`). The script prints, in order: the host's clock (`date +%s`), one
//! `NAME:WINDOWS:ATTACHED:ACTIVITY` line per window, one `tool NAME:COMMAND` line per pane, `---`,
//! then Claude Code's status file of every live Claude session, each followed by `failed` (its
//! newest reply is an API error) and `usage …` (model and token counts of its newest real reply).
//! When tmux cannot list, the window lines are replaced by one `tmux-error MESSAGE` line.
//!
//! A Claude session whose status file names no tmux pane runs outside tmux (in a terminal
//! window of its own). The snapshot has those too, after the tmux sessions, under the host
//! name [`KITTY`] as in tmls (`local.py`): they have a status but nothing to attach to.

use serde_json::Value;

/// The `host` of a Claude session that runs outside tmux (tmls `hosts.KITTY`). Such a session
/// is in the snapshot of the machine it runs on; its [`SessionInfo::pid`] is Claude's process.
pub const KITTY: &str = "kitty";

/// Seconds without output before a session that is not Claude counts as idle.
const QUIET_S: u64 = 30;

/// A session's identity. `host` is never empty: this machine is its `Config::local_name`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey {
    pub host: String,
    pub name: String,
}

/// What a session is doing. The parser never returns `Unseen`: `seen.rs` sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Waiting,
    Unseen,
    Error,
    Idle,
}

impl Status {
    /// The mark tmls draws after the session's name.
    pub fn mark(self) -> char {
        match self {
            Status::Running => '●',
            Status::Waiting => '?',
            Status::Unseen => '◆',
            Status::Error => '✕',
            Status::Idle => '○',
        }
    }
}

/// The agent CLI a session runs; `Other` is a plain shell or anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Opencode,
    Codex,
    Other,
}

impl Tool {
    /// The letter tmls draws between a session's mark and its name.
    pub fn letter(self) -> Option<char> {
        match self {
            Tool::Claude => Some('C'),
            Tool::Opencode => Some('O'),
            Tool::Codex => Some('X'),
            Tool::Other => None,
        }
    }

    /// From a pane's foreground command.
    fn of_command(command: &str) -> Option<Tool> {
        match command {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            "opencode" => Some(Tool::Opencode),
            _ => None,
        }
    }

    /// Which tool names a session with several: lower wins (tmls: claude, codex, opencode).
    fn precedence(self) -> u8 {
        match self {
            Tool::Claude => 0,
            Tool::Codex => 1,
            Tool::Opencode => 2,
            Tool::Other => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub key: SessionKey,
    pub status: Status,
    /// What Claude waits on you for ("permission prompt", "input needed"); only with `Waiting`.
    pub reason: Option<String>,
    pub tool: Tool,
    /// Claude's name for the conversation.
    pub title: Option<String>,
    /// How full Claude's context is.
    pub context_pct: Option<u8>,
    /// Attached tmux clients.
    pub attached: u32,
    /// Seconds since the session's newest window output, by the host's own clock.
    pub last_output_age_s: Option<u64>,
    /// Claude's process, for a session outside tmux ([`KITTY`]); `None` for a tmux session.
    pub pid: Option<u32>,
    /// Claude's id of the conversation, which names its transcript
    /// (`~/.claude/projects/*/ID.jsonl`); `None` for a session that is not Claude's, and for an
    /// id that is not one ([`crate::files::valid_session_id`]): it becomes part of a command.
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostSnapshot {
    pub host: String,
    /// The host answered with a listing. A host with no tmux server is online with no sessions.
    pub online: bool,
    /// Why the host is offline, when that is known.
    pub error: Option<String>,
    pub sessions: Vec<SessionInfo>,
}

/// Lists one host. POSIX sh, read from stdin by `sh -s` (see [`listing_command`]); the body is
/// one compound command with stdin on /dev/null, so nothing in it can swallow the script's tail.
pub const LISTING_SCRIPT: &str = r##"{
date +%s
if w=$(tmux -u list-windows -a -F '#{session_name}:#{session_windows}:#{session_attached}:#{window_activity}' 2>&1); then
  printf '%s\n' "$w"
  tmux -u list-panes -a -F '#{session_name}:#{pane_current_command}:#{pane_tty}' | while IFS= read -r l; do
    t=${l##*:}; r=${l%:*}; c=${r##*:}
    [ "$c" = node ] && ps -o args= -t "$t" 2>/dev/null | grep -q codex && c=codex
    echo "tool ${r%:*}:$c"
  done
  echo ---
  for f in "$HOME"/.claude/sessions/*.json; do
    p=${f##*/}
    kill -0 "${p%.json}" 2>/dev/null || continue
    cat "$f"; echo
    i=$(sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p' "$f")
    for t in "$HOME"/.claude/projects/*/"$i".jsonl; do
      [ -f "$t" ] || continue
      a=$(tail -c 300000 "$t" | grep '[^\\]"type":"assistant"')
      printf '%s\n' "$a" | tail -n 1 | grep -q '[^\\]"isApiErrorMessage":true' && echo failed
      echo usage $(printf '%s\n' "$a" | grep -v '[^\\]"isApiErrorMessage":true' | tail -n 1 | grep -o '"model":"[^"]*"\|"[a-z_]*input_tokens":[0-9]*' | head -n 4)
    done
  done
else
  printf 'tmux-error %s\n' "$w"
fi
true
} </dev/null
"##;

/// The command that lists `host`; the caller writes [`LISTING_SCRIPT`] to its stdin.
/// This machine (`host == local_name`) runs `sh -s`; any other host runs it over ssh, which
/// must not prompt (`BatchMode`) and gives up connecting after 5 s.
pub fn listing_command(host: &str, local_name: &str) -> std::process::Command {
    if host == local_name {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-s");
        cmd
    } else {
        let mut cmd = std::process::Command::new("ssh");
        cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, "sh", "-s"]);
        cmd
    }
}

/// One Claude Code session, from its status file and the lines the script printed after it.
struct Claude {
    session: String,
    status: Option<String>,
    since: i64,
    waiting: Option<String>,
    failed: bool,
    title: Option<String>,
    model: Option<String>,
    context: u64,
    pid: Option<u32>,
    /// `kind` is `interactive`: a person's session, not a helper Claude started itself.
    interactive: bool,
    /// The last part of its working directory: its name when it has none.
    folder: Option<String>,
    /// `sessionId`, if it is one.
    id: Option<String>,
}

impl Claude {
    fn from_json(line: &str) -> Option<Claude> {
        let value: Value = serde_json::from_str(line).ok()?;
        let file = value.as_object()?;
        let text = |key: &str| file.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let pane = file.get("tmux").and_then(Value::as_str).unwrap_or("");
        let updated = file.get("statusUpdatedAt");
        let millis =
            updated.and_then(Value::as_i64).or_else(|| updated.and_then(Value::as_f64).map(|f| f as i64)).unwrap_or(0);
        Some(Claude {
            // "SESSION:@WINDOW.%PANE"
            session: pane.rsplit_once(':').map_or(pane, |(session, _)| session).to_string(),
            status: text("status"),
            since: millis.div_euclid(1000),
            waiting: text("waitingFor"),
            failed: false,
            title: text("name"),
            model: None,
            context: 0,
            pid: file.get("pid").and_then(Value::as_u64).and_then(|pid| u32::try_from(pid).ok()),
            interactive: file.get("kind").and_then(Value::as_str) == Some("interactive"),
            folder: text("cwd").and_then(|cwd| cwd.rsplit('/').find(|part| !part.is_empty()).map(str::to_string)),
            id: text("sessionId").filter(|id| crate::files::valid_session_id(id)),
        })
    }

    /// Reads a `usage "model":"…" "input_tokens":N …` line.
    fn read_usage(&mut self, line: &str) {
        let mut model = None;
        let mut tokens = 0u64;
        for field in line.split_whitespace().skip(1) {
            let Some((key, value)) = field.split_once(':') else { continue };
            let value = value.trim_matches('"');
            match key.trim_matches('"') {
                "model" => model = Some(value.to_string()),
                "input_tokens" | "cache_creation_input_tokens" | "cache_read_input_tokens" => {
                    tokens = tokens.saturating_add(value.parse().unwrap_or(0));
                }
                _ => {}
            }
        }
        if let Some(model) = model.filter(|m| !m.is_empty()) {
            self.model = Some(model);
            self.context = tokens;
        }
    }

    fn running(&self) -> bool {
        // "shell": the turn is over but a monitor or background shell still runs
        matches!(self.status.as_deref(), Some("busy" | "shell"))
    }

    fn waits(&self) -> bool {
        self.status.as_deref() == Some("waiting")
    }

    /// Two Claudes in one session: waiting on you wins, then running, then the latest change.
    fn rank(&self) -> (bool, bool, i64) {
        (self.waits(), self.running(), self.since)
    }

    /// Claude Code doesn't save its context limit: Claude 5 models run with 1M, older ones 200k.
    fn context_pct(&self) -> Option<u8> {
        let model = self.model.as_deref()?;
        let generation = model.strip_prefix("claude-").and_then(|rest| {
            let (family, version) = rest.split_once('-')?;
            if family.is_empty() || !family.bytes().all(|b| b.is_ascii_lowercase()) {
                return None;
            }
            let digits = version.bytes().take_while(u8::is_ascii_digit).count();
            version[..digits].parse::<u64>().ok()
        });
        let limit: u64 = if generation.is_some_and(|g| g >= 5) { 1_000_000 } else { 200_000 };
        Some((self.context.saturating_mul(100) / limit).min(100) as u8)
    }
}

struct Window<'a> {
    name: &'a str,
    attached: u32,
    activity: u64,
}

/// `NAME:WINDOWS:ATTACHED:ACTIVITY`, split from the right: tmux bans ":" in names, a login
/// script's line is not bound by that.
fn window_line(line: &str) -> Option<Window<'_>> {
    let mut fields = line.rsplitn(4, ':');
    let (activity, attached, windows, name) = (fields.next()?, fields.next()?, fields.next()?, fields.next()?);
    windows.parse::<u64>().ok()?;
    let activity = activity.parse().ok()?;
    if name.is_empty() {
        return None;
    }
    let attached = if attached == "0" { 0 } else { attached.parse().unwrap_or(1) };
    Some(Window { name, attached, activity })
}

fn offline(host: &str, error: &str) -> HostSnapshot {
    HostSnapshot { host: host.to_string(), online: false, error: Some(error.to_string()), sessions: Vec::new() }
}

/// Reads what [`LISTING_SCRIPT`] printed on `host`. Total: any input gives a snapshot. Output
/// without a clock line (a login script's text only, nothing at all) is an offline host; lines
/// that are not part of the listing are skipped.
pub fn parse_listing(host: &str, output: &str) -> HostSnapshot {
    let (tmux, claude) = output.split_once("\n---\n").unwrap_or((output, ""));
    let lines: Vec<&str> = tmux.lines().collect();
    // the last all-digit line: a login banner prints before `date`, and window lines have colons
    let clock = lines.iter().enumerate().rev().find_map(|(i, line)| {
        let digits = !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit());
        Some((i, line.parse::<u64>().ok().filter(|_| digits)?))
    });
    let Some((start, now)) = clock else {
        return offline(host, "no listing from the host");
    };

    let mut windows: Vec<Window> = Vec::new(); // one per session, in the order tmux lists them
    let mut tools: Vec<(&str, Tool)> = Vec::new();
    let mut tmux_error = None;
    for line in &lines[start + 1..] {
        if let Some(window) = window_line(line) {
            match windows.iter_mut().find(|w| w.name == window.name) {
                Some(known) => known.activity = known.activity.max(window.activity), // newest window output wins
                None => windows.push(window),
            }
        } else if let Some((name, command)) = line.strip_prefix("tool ").and_then(|rest| rest.rsplit_once(':')) {
            // not a window line; a session can't be named "tool x:y" and still parse as one above
            if let Some(tool) = Tool::of_command(command) {
                match tools.iter_mut().find(|(known, _)| *known == name) {
                    Some((_, known)) if tool.precedence() < known.precedence() => *known = tool,
                    Some(_) => {}
                    None => tools.push((name, tool)),
                }
            }
        } else if let Some(message) = line.strip_prefix("tmux-error ") {
            tmux_error.get_or_insert(message);
        }
    }
    if let (Some(message), true) = (tmux_error, windows.is_empty()) {
        // tmux < 3.7: "no server running"; 3.7: "error connecting to <socket> (No such file or directory)"
        if message.contains("no server running") || message.contains("error connecting to") {
            return HostSnapshot { host: host.to_string(), online: true, error: None, sessions: Vec::new() };
        }
        return offline(host, message);
    }

    let mut files: Vec<Claude> = Vec::new();
    let mut skipping = false; // the last file didn't parse (caught mid-write): its lines aren't the previous one's
    for line in claude.lines() {
        if line.trim().is_empty() {
            continue; // `cat "$f"; echo` after a file that ends in a newline
        }
        if line == "failed" || line.starts_with("usage") {
            if let (Some(file), false) = (files.last_mut(), skipping) {
                if line == "failed" {
                    file.failed = true;
                } else {
                    file.read_usage(line);
                }
            }
            continue;
        }
        match Claude::from_json(line) {
            Some(file) => {
                files.push(file);
                skipping = false;
            }
            None => skipping = true,
        }
    }

    let sessions = windows
        .iter()
        .map(|w| {
            let mut claude: Option<&Claude> = None;
            for file in files.iter().filter(|f| f.session == w.name) {
                if claude.is_none_or(|best| best.status.is_none() || file.rank() > best.rank()) {
                    claude = Some(file);
                }
            }
            let age = now.saturating_sub(w.activity);
            let status = match claude.filter(|c| c.status.is_some()) {
                Some(c) if c.waits() => Status::Waiting,
                Some(c) if c.running() => Status::Running,
                Some(c) if c.failed => Status::Error,
                Some(_) => Status::Idle, // its status line redraws every minute: output isn't work
                None if age < QUIET_S => Status::Running,
                None => Status::Idle,
            };
            let tool = match claude {
                Some(_) => Tool::Claude, // its session file says so, whatever the pane's command is called
                None => tools.iter().find(|(name, _)| *name == w.name).map_or(Tool::Other, |(_, tool)| *tool),
            };
            SessionInfo {
                key: SessionKey { host: host.to_string(), name: w.name.to_string() },
                status,
                reason: claude.filter(|_| status == Status::Waiting).and_then(|c| c.waiting.clone()),
                tool,
                title: claude.and_then(|c| c.title.clone()),
                context_pct: claude.and_then(Claude::context_pct),
                attached: w.attached,
                last_output_age_s: Some(age),
                pid: None,
                session_id: claude.and_then(|c| c.id.clone()),
            }
        })
        .collect();
    let mut sessions: Vec<SessionInfo> = sessions;
    // Claude outside tmux (tmls `local.to_sessions`): named by Claude, else by its folder, else
    // by its process; a name taken already gets the process added.
    for file in files.iter().filter(|f| f.session.is_empty() && f.interactive) {
        let Some(pid) = file.pid else { continue };
        let mut name = file.title.clone().or_else(|| file.folder.clone()).unwrap_or_else(|| pid.to_string());
        if sessions.iter().any(|s| s.key.host == KITTY && s.key.name == name) {
            name = format!("{name} ({pid})");
        }
        let status = match () {
            () if file.waits() => Status::Waiting,
            () if file.running() => Status::Running,
            () if file.failed => Status::Error,
            () => Status::Idle,
        };
        sessions.push(SessionInfo {
            key: SessionKey { host: KITTY.to_string(), name },
            status,
            reason: file.waiting.clone().filter(|_| status == Status::Waiting),
            tool: Tool::Claude,
            title: None, // its name is Claude's title already
            context_pct: file.context_pct(),
            attached: 0,
            last_output_age_s: None,
            pid: Some(pid),
            session_id: file.id.clone(),
        });
    }
    HostSnapshot { host: host.to_string(), online: true, error: None, sessions }
}
