//! What an agent CLI other than Claude is doing, read off its pane: herdr's detection rules.
//!
//! herdr (Apache-2.0, <https://github.com/herdrdev/herdr>) keeps one TOML file of rules per agent
//! in `distribution/agent-detection`; `agent-detection/` beside this crate is a copy of it (see
//! its `NOTICE` for the commit). Each rule has a state (`idle`, `working`, `blocked` or
//! `unknown`), a priority, a region of the pane's text it looks at, and matchers on that text.
//! This is a port of herdr's engine (`src/detect/manifest.rs`, engine version 3) and keeps its
//! meaning exactly: every rule whose matchers hold matches, the highest priority wins and a tie
//! goes to the rule that comes first in the file; when no rule matches, a known agent is idle.
//! `contains` is case-insensitive, `regex` must match somewhere in the region, `line_regex`
//! on some line of it; `all`, `any` and `not` nest.
//!
//! The text is the pane's visible screen as `tmux capture-pane -p` prints it, made the way herdr
//! makes its own ([`screen_text`]); the title is the pane's `#{pane_title}`, which is what the
//! agent set with OSC 0/2 (herdr's `osc_title`). tmux keeps no OSC 9 progress, so the
//! `osc_progress` region is always empty here.

use std::sync::OnceLock;

use regex::Regex;
use toml_edit::{Item, TableLike, Value};

/// The rule-file format this engine reads; files that need a newer one are left out.
pub const ENGINE_VERSION: u32 = 3;

/// The herdr commit `agent-detection/` was copied from.
pub const HERDR_COMMIT: &str = "916fcf8de029c6cac965b50f3cdb9687cd7d1537";

/// herdr's `index.toml`: which file is which agent's.
pub const INDEX: &str = include_str!("../agent-detection/index.toml");

/// Every rule file, by its agent's id (the `id` in `index.toml` and in the file).
pub const RULE_FILES: &[(&str, &str)] = &[
    ("amp", include_str!("../agent-detection/amp.toml")),
    ("agy", include_str!("../agent-detection/antigravity.toml")),
    ("claude", include_str!("../agent-detection/claude.toml")),
    ("cline", include_str!("../agent-detection/cline.toml")),
    ("codex", include_str!("../agent-detection/codex.toml")),
    ("cursor", include_str!("../agent-detection/cursor.toml")),
    ("devin", include_str!("../agent-detection/devin.toml")),
    ("droid", include_str!("../agent-detection/droid.toml")),
    ("gemini", include_str!("../agent-detection/gemini.toml")),
    ("grok", include_str!("../agent-detection/grok.toml")),
    ("hermes", include_str!("../agent-detection/hermes.toml")),
    ("kilo", include_str!("../agent-detection/kilo.toml")),
    ("kimi", include_str!("../agent-detection/kimi.toml")),
    ("kiro", include_str!("../agent-detection/kiro.toml")),
    ("letta", include_str!("../agent-detection/letta.toml")),
    ("maki", include_str!("../agent-detection/maki.toml")),
    ("muse", include_str!("../agent-detection/muse.toml")),
    ("opencode", include_str!("../agent-detection/opencode.toml")),
    ("pi", include_str!("../agent-detection/pi.toml")),
    ("qodercli", include_str!("../agent-detection/qodercli.toml")),
    ("qwen", include_str!("../agent-detection/qwen.toml")),
    ("copilot", include_str!("../agent-detection/github-copilot.toml")),
];

/// What a pane's agent is doing, by its rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Working,
    /// It waits on you: a permission prompt, a question, a trust dialog.
    Blocked,
    /// The screen says nothing about it (herdr: keep the state it had).
    Unknown,
}

/// The outcome of one pane's rules: its state and the rule that decided it, `None` when no rule
/// matched and the agent is idle by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detection {
    pub state: AgentState,
    pub rule: Option<&'static str>,
    /// The rule says the screen shows no live state (a transcript viewer): herdr leaves the
    /// pane's state as it was.
    pub skip_state_update: bool,
}

/// One agent's rules, parsed and compiled.
#[derive(Debug)]
pub struct Rules {
    pub id: String,
    pub version: Option<String>,
    pub min_engine_version: Option<u32>,
    pub aliases: Vec<String>,
    rules: Vec<Rule>,
}

#[derive(Debug)]
struct Rule {
    id: String,
    state: AgentState,
    priority: i64,
    region: Region,
    skip_state_update: bool,
    gate: Gate,
}

#[derive(Debug, Default)]
struct Gate {
    all: Vec<Gate>,
    any: Vec<Gate>,
    not: Vec<Gate>,
    /// Lowercased, as herdr compiles them; matched against the lowercased region.
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    WholeRecent,
    AfterLastPromptMarker,
    BeforeCurrentPromptMarker,
    WholeRecentWithoutCurrentPromptMarker,
    CurrentPromptBlockMarker,
    AfterCurrentPromptBlockMarker,
    PromptBoxBody,
    AbovePromptBox,
    LastNonEmptyAbovePromptBox,
    AfterLastHorizontalRule,
    OscTitle,
    OscProgress,
    BottomLines(usize),
    BottomNonEmptyLines(usize),
    TopNonEmptyLines(usize),
}

impl Region {
    fn parse(spec: &str) -> Option<Region> {
        let spec = spec.trim();
        let count = |name: &str| {
            spec.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?.parse::<usize>().ok()
        };
        Some(match spec {
            "whole_recent" => Region::WholeRecent,
            "after_last_prompt_marker" => Region::AfterLastPromptMarker,
            "before_current_prompt_marker" => Region::BeforeCurrentPromptMarker,
            "whole_recent_without_current_prompt_marker" => Region::WholeRecentWithoutCurrentPromptMarker,
            "current_prompt_block_marker" => Region::CurrentPromptBlockMarker,
            "after_current_prompt_block_marker" => Region::AfterCurrentPromptBlockMarker,
            "prompt_box_body" => Region::PromptBoxBody,
            "above_prompt_box" => Region::AbovePromptBox,
            "last_non_empty_above_prompt_box" => Region::LastNonEmptyAbovePromptBox,
            "after_last_horizontal_rule" => Region::AfterLastHorizontalRule,
            "osc_title" => Region::OscTitle,
            "osc_progress" => Region::OscProgress,
            _ => {
                if let Some(n) = count("bottom_lines") {
                    Region::BottomLines(n)
                } else if let Some(n) = count("bottom_non_empty_lines") {
                    Region::BottomNonEmptyLines(n)
                } else {
                    // herdr: no leading zero, at most u16::MAX
                    let n = spec.strip_prefix("top_non_empty_lines")?.strip_prefix('(')?.strip_suffix(')')?;
                    if n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
                        return None;
                    }
                    Region::TopNonEmptyLines(n.parse::<usize>().ok().filter(|n| *n <= u16::MAX as usize)?)
                }
            }
        })
    }

    /// The part of `screen` (or the title) this region is, as herdr's `region` cuts it.
    fn of<'a>(self, screen: &'a str, title: &'a str) -> &'a str {
        match self {
            Region::OscTitle => title,
            Region::OscProgress => "",
            Region::WholeRecent => screen,
            Region::AfterLastPromptMarker => after_last_prompt_marker(screen),
            Region::BeforeCurrentPromptMarker => before_current_prompt_marker(screen),
            Region::WholeRecentWithoutCurrentPromptMarker => {
                if current_prompt_index(&screen.lines().collect::<Vec<_>>()).is_some() { "" } else { screen }
            }
            Region::CurrentPromptBlockMarker => current_prompt_block_marker(screen).unwrap_or(""),
            Region::AfterCurrentPromptBlockMarker => after_current_prompt_block_marker(screen).unwrap_or(""),
            Region::PromptBoxBody => prompt_box_body(screen).unwrap_or(""),
            Region::AbovePromptBox => above_prompt_box(screen),
            Region::LastNonEmptyAbovePromptBox => {
                above_prompt_box(screen).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("")
            }
            Region::AfterLastHorizontalRule => after_last_horizontal_rule(screen),
            Region::BottomLines(n) => {
                let lines: Vec<&str> = screen.lines().collect();
                from_line(screen, &lines, lines.len().saturating_sub(n))
            }
            Region::BottomNonEmptyLines(n) => {
                let lines: Vec<&str> = screen.lines().collect();
                let start = lines.iter().enumerate().rev().filter(|(_, l)| !l.trim().is_empty()).take(n).last();
                start.map_or("", |(i, _)| from_line(screen, &lines, i))
            }
            Region::TopNonEmptyLines(n) => {
                let lines: Vec<&str> = screen.lines().collect();
                let end = lines.iter().enumerate().filter(|(_, l)| !l.trim().is_empty()).take(n).last();
                end.map_or("", |(i, _)| &screen[..line_start(screen, &lines, i + 1)])
            }
        }
    }
}

impl Gate {
    fn matches(&self, text: &str, lower: &str) -> bool {
        self.contains.iter().all(|needle| lower.contains(needle.as_str()))
            && self.regex.iter().all(|re| re.is_match(text))
            && self.line_regex.iter().all(|re| text.lines().any(|line| re.is_match(line)))
            && self.all.iter().all(|gate| gate.matches(text, lower))
            && (self.any.is_empty() || self.any.iter().any(|gate| gate.matches(text, lower)))
            && !self.not.iter().any(|gate| gate.matches(text, lower))
    }

    fn positive(&self) -> bool {
        !(self.contains.is_empty()
            && self.regex.is_empty()
            && self.line_regex.is_empty()
            && self.all.is_empty()
            && self.any.is_empty())
    }
}

impl Rules {
    /// What the pane shows, by these rules: `screen` is [`screen_text`] of its capture, `title`
    /// its pane title.
    pub fn detect(&'static self, screen: &str, title: &str) -> Detection {
        let mut best: Option<&'static Rule> = None;
        for rule in &self.rules {
            let text = rule.region.of(screen, title);
            if !rule.gate.matches(text, &text.to_lowercase()) {
                continue;
            }
            if best.is_none_or(|b| rule.priority > b.priority) {
                best = Some(rule);
            }
        }
        match best {
            Some(rule) => Detection { state: rule.state, rule: Some(&rule.id), skip_state_update: rule.skip_state_update },
            None => Detection { state: AgentState::Idle, rule: None, skip_state_update: false },
        }
    }

    /// The process names herdr knows this agent by: its id and its aliases, less the ones that
    /// cannot be one (`herdr:opencode`, `kimi code`).
    pub fn process_names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str())
            .chain(self.aliases.iter().map(String::as_str))
            .filter(|name| !name.contains([' ', ':']))
    }
}

/// Parses and checks one rule file as herdr does (`deny_unknown_fields`, `validate_manifest`):
/// unknown keys, a rule with no positive matcher, an empty `not`, a bad region or regex, and
/// `skip_state_update` on a rule that is not `unknown` are errors. A file that needs a newer
/// engine than [`ENGINE_VERSION`] is one too.
pub fn parse_rules(text: &str) -> Result<Rules, String> {
    let doc = toml_edit::Document::parse(text).map_err(|e| e.to_string())?;
    let top = doc.as_table();
    known_keys(top, &["id", "version", "min_engine_version", "updated_at", "aliases", "rules"], "file")?;
    let id = top.get("id").and_then(Item::as_str).ok_or("no id")?.to_string();
    let version = top.get("version").map(|v| v.as_str().map(str::to_string).ok_or("version is not a string")).transpose()?;
    let min_engine_version = match top.get("min_engine_version") {
        Some(v) => Some(v.as_integer().and_then(|n| u32::try_from(n).ok()).ok_or("min_engine_version is not a number")?),
        None => None,
    };
    if min_engine_version.is_some_and(|v| v > ENGINE_VERSION) {
        return Err(format!("needs engine {}, this is {ENGINE_VERSION}", min_engine_version.unwrap_or(0)));
    }
    let aliases = match top.get("aliases") {
        Some(item) => strings(item, "aliases")?,
        None => Vec::new(),
    };
    let mut rules = Vec::new();
    for table in tables(top.get("rules"), "rules")? {
        let rule_id = table.get("id").and_then(Item::as_str).filter(|s| !s.trim().is_empty()).ok_or("a rule has no id")?;
        let err = |e: String| format!("rule {rule_id}: {e}");
        let mut keys = vec!["id", "state", "priority", "region", "visible_idle", "visible_blocker", "visible_working"];
        keys.extend(["skip_state_update", "all", "any", "not", "contains", "regex", "line_regex"]);
        known_keys(table, &keys, "rule").map_err(err)?;
        let state = match table.get("state").map(|s| s.as_str()) {
            None => AgentState::Unknown,
            Some(Some("idle")) => AgentState::Idle,
            Some(Some("working")) => AgentState::Working,
            Some(Some("blocked")) => AgentState::Blocked,
            Some(Some("unknown")) => AgentState::Unknown,
            Some(other) => return Err(err(format!("bad state {other:?}"))),
        };
        let flag = |key: &str| -> Result<bool, String> {
            table.get(key).map_or(Ok(false), |v| v.as_bool().ok_or_else(|| err(format!("{key} is not a boolean"))))
        };
        let skip_state_update = flag("skip_state_update")?;
        let visible = flag("visible_idle")? || flag("visible_blocker")? || flag("visible_working")?;
        if skip_state_update && (state != AgentState::Unknown || table.get("state").is_none() || visible) {
            return Err(err("skip_state_update needs state = \"unknown\" and no visible_* flag".into()));
        }
        let priority = table.get("priority").map_or(Ok(0), |p| p.as_integer().ok_or_else(|| err("priority is not a number".into())))?;
        let region_spec = table.get("region").map_or(Ok("whole_recent"), |r| r.as_str().ok_or_else(|| err("region is not a string".into())))?;
        let region = Region::parse(region_spec).ok_or_else(|| err(format!("bad region {region_spec:?}")))?;
        if matches!(region, Region::TopNonEmptyLines(_)) && min_engine_version.is_some_and(|v| v < 3) {
            return Err(err("top_non_empty_lines needs min_engine_version 3".into()));
        }
        let gate = gate(table, false).map_err(err)?;
        rules.push(Rule { id: rule_id.to_string(), state, priority, region, skip_state_update, gate });
    }
    if rules.is_empty() {
        return Err("no rules".into());
    }
    Ok(Rules { id, version, min_engine_version, aliases, rules })
}

/// A rule's or nested gate's matchers. A `not` gate may hold only further `not`s; any other
/// needs a positive matcher.
fn gate(table: &dyn TableLike, negated: bool) -> Result<Gate, String> {
    let nested = |key: &str, negated: bool| -> Result<Vec<Gate>, String> {
        let mut out = Vec::new();
        for t in tables(table.get(key), key)? {
            known_keys(t, &["all", "any", "not", "contains", "regex", "line_regex"], "gate")?;
            out.push(gate(t, negated)?);
        }
        Ok(out)
    };
    let regexes = |key: &str| -> Result<Vec<Regex>, String> {
        let patterns = table.get(key).map_or(Ok(Vec::new()), |item| strings(item, key))?;
        patterns.iter().map(|p| Regex::new(p).map_err(|e| format!("bad {key} {p:?}: {e}"))).collect()
    };
    let contains = table.get("contains").map_or(Ok(Vec::new()), |item| strings(item, "contains"))?;
    let g = Gate {
        all: nested("all", false)?,
        any: nested("any", false)?,
        not: nested("not", true)?,
        contains: contains.iter().map(|s| s.to_lowercase()).collect(),
        regex: regexes("regex")?,
        line_regex: regexes("line_regex")?,
    };
    let empty = !g.positive() && g.not.is_empty();
    if (!negated && !g.positive()) || empty {
        return Err("a gate has no matcher".into());
    }
    Ok(g)
}

fn known_keys(table: &dyn TableLike, keys: &[&str], what: &str) -> Result<(), String> {
    match table.iter().find(|(key, _)| !keys.contains(key)) {
        Some((key, _)) => Err(format!("unknown {what} key {key:?}")),
        None => Ok(()),
    }
}

fn strings(item: &Item, key: &str) -> Result<Vec<String>, String> {
    let array = item.as_array().ok_or_else(|| format!("{key} is not a list"))?;
    array.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{key} holds a non-string"))).collect()
}

/// `[[key]]` tables or a list of inline tables; nothing when the key is absent.
fn tables<'a>(item: Option<&'a Item>, key: &str) -> Result<Vec<&'a dyn TableLike>, String> {
    match item {
        None => Ok(Vec::new()),
        Some(Item::ArrayOfTables(array)) => Ok(array.iter().map(|t| t as &dyn TableLike).collect()),
        Some(Item::Value(Value::Array(array))) => array
            .iter()
            .map(|v| v.as_inline_table().map(|t| t as &dyn TableLike).ok_or_else(|| format!("{key} holds a non-table")))
            .collect(),
        Some(_) => Err(format!("{key} is not a list of tables")),
    }
}

/// The rules of agent `id`, parsed on first use; `None` for an agent herdr has no file for, or
/// whose file does not parse here.
pub fn rules(id: &str) -> Option<&'static Rules> {
    static PARSED: [OnceLock<Option<Rules>>; RULE_FILES.len()] = [const { OnceLock::new() }; RULE_FILES.len()];
    let i = RULE_FILES.iter().position(|(known, _)| *known == id)?;
    PARSED[i].get_or_init(|| parse_rules(RULE_FILES[i].1).ok()).as_ref()
}

/// The agent a pane's foreground command is (`gemini`, `kiro-cli`), by the ids and aliases in
/// the rule files; `None` for anything else.
pub fn agent_of_command(command: &str) -> Option<&'static str> {
    static NAMES: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let mut names = Vec::new();
        for (id, text) in RULE_FILES {
            let Ok(doc) = toml_edit::Document::parse(*text) else { continue };
            names.push((id.to_string(), *id));
            let aliases = doc.as_table().get("aliases").and_then(|a| strings(a, "aliases").ok()).unwrap_or_default();
            names.extend(aliases.into_iter().filter(|a| !a.contains([' ', ':'])).map(|a| (a, *id)));
        }
        names
    });
    names.iter().find(|(name, _)| name == command).map(|(_, id)| *id)
}

/// Agent `id`'s state on a pane whose capture is `lines` and whose title is `title`; `None`
/// when herdr has no rules for it.
pub fn detect(id: &str, lines: &[&str], title: &str) -> Option<Detection> {
    Some(rules(id)?.detect(&screen_text(lines), title))
}

/// A capture as herdr's detection text: each line without its trailing blanks, no blank lines
/// at the end, joined by newlines with one after the last.
pub fn screen_text(lines: &[&str]) -> String {
    let mut rows: Vec<&str> = lines.iter().map(|line| line.trim_end()).collect();
    while rows.last().is_some_and(|row| row.trim().is_empty()) {
        rows.pop();
    }
    if rows.is_empty() { String::new() } else { rows.join("\n") + "\n" }
}

// The regions below are herdr's, function for function.

fn after_last_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    match lines.iter().rposition(|line| prompt_line(line)) {
        Some(i) => from_line(content, &lines, i + 1),
        None => content,
    }
}

fn before_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(i) = current_prompt_index(&lines) else { return content };
    let end: usize = lines[..i].iter().map(|line| line.len() + 1).sum();
    &content[..end.min(content.len())]
}

fn current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt = current_prompt_index(&lines)?;
    lines[..prompt].iter().rev().find(|line| block_marker_line(line)).copied()
}

fn after_current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt = current_prompt_index(&lines)?;
    let block = lines[..prompt].iter().rposition(|line| block_marker_line(line))?;
    Some(from_line(content, &lines, block))
}

/// Codex's last `›` prompt line, unless a response block starts after it.
fn current_prompt_index(lines: &[&str]) -> Option<usize> {
    let prompt = lines.iter().rposition(|line| prompt_line(line))?;
    if lines[prompt + 1..].iter().any(|line| block_marker_line(line)) {
        return None;
    }
    Some(prompt)
}

fn prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn block_marker_line(line: &str) -> bool {
    line.starts_with(['•', '■', '✗', '✓'])
}

fn prompt_box_body(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let top = prompt_box_top(&lines)?;
    let start = line_start(content, &lines, top + 1);
    let end = lines[top + 1..].iter().position(|line| horizontal_rule(line)).map_or(lines.len(), |i| top + 1 + i);
    let end = line_start(content, &lines, end);
    Some(&content[start.min(content.len())..end.min(content.len())])
}

fn above_prompt_box(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    match prompt_box_top(&lines) {
        Some(top) => &content[..line_start(content, &lines, top).min(content.len())],
        None => content,
    }
}

fn after_last_horizontal_rule(content: &str) -> &str {
    let (mut last_end, mut offset) = (0usize, 0usize);
    for line in content.lines() {
        let next = offset + line.len() + 1;
        if horizontal_rule(line) {
            last_end = next.min(content.len());
        }
        offset = next;
    }
    &content[last_end..]
}

/// The second horizontal rule from the bottom: the top border of a prompt box.
fn prompt_box_top(lines: &[&str]) -> Option<usize> {
    lines.iter().enumerate().rev().filter(|(_, line)| horizontal_rule(line)).nth(1).map(|(i, _)| i)
}

/// A line of `─`, or one that starts with at least three and has a label after them.
fn horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let rule_chars = trimmed.chars().take_while(|&ch| ch == '─').count();
    if rule_chars == 0 {
        return false;
    }
    let rule_bytes = trimmed.char_indices().nth(rule_chars).map_or(trimmed.len(), |(i, _)| i);
    trimmed[rule_bytes..].trim_start().is_empty() || rule_chars >= 3
}

fn from_line<'a>(content: &'a str, lines: &[&str], index: usize) -> &'a str {
    &content[line_start(content, lines, index).min(content.len())..]
}

fn line_start(content: &str, lines: &[&str], index: usize) -> usize {
    lines[..index.min(lines.len())].iter().map(|line| line.len() + 1).sum::<usize>().min(content.len())
}
