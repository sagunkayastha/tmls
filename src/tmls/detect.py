"""What an agent CLI other than Claude is doing, read off its pane: herdr's detection rules.

herdr (Apache-2.0, https://github.com/herdrdev/herdr) keeps one TOML file of rules per agent in
`distribution/agent-detection`; `agent_detection/` beside this module is a copy of it (see its
`NOTICE` for the commit). Each rule has a state (`idle`, `working`, `blocked` or `unknown`), a
priority, a region of the pane's text it looks at, and matchers on that text.

A port of tmls-rs's port (`crates/client-core/src/detect.rs`) of herdr's engine
(`src/detect/manifest.rs`, engine version 3), with the same meaning: every rule whose matchers
hold matches, the highest priority wins and a tie goes to the rule that comes first in the file;
when no rule matches, a known agent is idle. `contains` is case-insensitive, `regex` must match
somewhere in the region, `line_regex` on some line of it; `all`, `any` and `not` nest.

The text is the pane's visible screen as `tmux capture-pane -p` prints it, made the way herdr
makes its own (`screen_text`); the title is the pane's `#{pane_title}` (herdr's `osc_title`).
tmux keeps no OSC 9 progress, so the `osc_progress` region is always empty here.

The patterns are written for Rust's regex crate; `_pattern` turns the few spellings Python's `re`
reads differently (`\\x{2801}`, `\\z`, `\\p{Alphabetic}`, `$` outside multi-line mode) into its own.
"""
import re
import tomllib
from dataclasses import dataclass, field
from functools import cache
from pathlib import Path

ENGINE_VERSION = 3
HERDR_COMMIT = "916fcf8de029c6cac965b50f3cdb9687cd7d1537"
RULES_DIR = Path(__file__).parent / "agent_detection"

# Every rule file, by its agent's id (the `id` in index.toml and in the file).
RULE_FILES = {
    "amp": "amp.toml", "agy": "antigravity.toml", "claude": "claude.toml", "cline": "cline.toml",
    "codex": "codex.toml", "cursor": "cursor.toml", "devin": "devin.toml", "droid": "droid.toml",
    "gemini": "gemini.toml", "grok": "grok.toml", "hermes": "hermes.toml", "kilo": "kilo.toml",
    "kimi": "kimi.toml", "kiro": "kiro.toml", "letta": "letta.toml", "maki": "maki.toml",
    "muse": "muse.toml", "opencode": "opencode.toml", "pi": "pi.toml", "qodercli": "qodercli.toml",
    "qwen": "qwen.toml", "copilot": "github-copilot.toml",
}

IDLE, WORKING, BLOCKED, UNKNOWN = "idle", "working", "blocked", "unknown"
STATES = (IDLE, WORKING, BLOCKED, UNKNOWN)


class RuleError(ValueError):
    pass


@dataclass(frozen=True)
class Detection:
    state: str
    rule: str | None
    # the rule says the screen shows no live state (a transcript viewer): herdr keeps the old state
    skip_state_update: bool = False


def _pattern(p):
    """A Rust regex as Python's re reads it the same way."""
    out, i, in_class = [], 0, False
    multiline = re.match(r"\(\?[a-zA-Z]*m", p) is not None or "(?m" in p
    while i < len(p):
        c = p[i]
        if c == "\\" and i + 1 < len(p):
            n = p[i + 1]
            m = re.match(r"([xuU])\{([0-9a-fA-F]+)\}", p[i + 1:])
            if m:
                out.append(f"\\U{int(m.group(2), 16):08x}")
                i += 1 + len(m.group(0))
                continue
            if n == "z":
                out.append(r"\Z")
            elif p.startswith("p{Alphabetic}", i + 1) and not in_class:
                out.append(r"[^\W\d_]")  # a letter (re has no \p; inside a class it is refused below)
                i += 1 + len("p{Alphabetic}")
                continue
            else:
                out.append(c + n)
            i += 2
            continue
        if in_class:
            if c == "]":
                in_class = False
        elif c == "[":
            in_class = True
            out.append(c)
            i += 1
            # a ] or ^] right after the opening bracket is a literal
            if p.startswith("^", i):
                out.append("^")
                i += 1
            if p.startswith("]", i):
                out.append(r"\]")
                i += 1
            continue
        elif c == "$" and not multiline:
            out.append(r"\Z")  # Rust: the end of the text only; Python's $ also matches before a last \n
            i += 1
            continue
        out.append(c)
        i += 1
    try:
        return re.compile("".join(out))
    except re.error as error:
        raise RuleError(f"bad regex {p!r}: {error}")


def _lines(text):
    """Rust's str::lines: split on \\n, no empty line after a final \\n, a trailing \\r dropped."""
    if not text:
        return []
    parts = text.split("\n")
    if parts[-1] == "":
        parts.pop()
    return [part[:-1] if part.endswith("\r") else part for part in parts]


@dataclass
class Gate:
    all: list = field(default_factory=list)
    any: list = field(default_factory=list)
    not_: list = field(default_factory=list)
    contains: list = field(default_factory=list)  # lowercased
    regex: list = field(default_factory=list)
    line_regex: list = field(default_factory=list)

    def matches(self, text, lower):
        return (all(needle in lower for needle in self.contains)
                and all(r.search(text) for r in self.regex)
                and all(any(r.search(line) for line in _lines(text)) for r in self.line_regex)
                and all(g.matches(text, lower) for g in self.all)
                and (not self.any or any(g.matches(text, lower) for g in self.any))
                and not any(g.matches(text, lower) for g in self.not_))

    def positive(self):
        return bool(self.contains or self.regex or self.line_regex or self.all or self.any)


@dataclass
class Rule:
    id: str
    state: str
    priority: int
    region: tuple
    skip_state_update: bool
    gate: Gate


@dataclass
class Rules:
    id: str
    version: str | None
    min_engine_version: int | None
    aliases: list
    rules: list

    def detect(self, screen, title):
        """What the pane shows: `screen` is screen_text of its capture, `title` its pane title."""
        best = None
        for rule in self.rules:
            text = region_of(rule.region, screen, title)
            if not rule.gate.matches(text, text.lower()):
                continue
            if best is None or rule.priority > best.priority:
                best = rule
        if best is None:
            return Detection(IDLE, None, False)
        return Detection(best.state, best.id, best.skip_state_update)

    def process_names(self):
        """The process names herdr knows this agent by, less the ones that cannot be one."""
        return [n for n in (self.id, *self.aliases) if " " not in n and ":" not in n]


def _known(table, keys, what):
    for key in table:
        if key not in keys:
            raise RuleError(f"unknown {what} key {key!r}")


def _strings(value, key):
    if not isinstance(value, list):
        raise RuleError(f"{key} is not a list")
    if not all(isinstance(v, str) for v in value):
        raise RuleError(f"{key} holds a non-string")
    return list(value)


def _tables(value, key):
    if value is None:
        return []
    if not isinstance(value, list):
        raise RuleError(f"{key} is not a list of tables")
    if not all(isinstance(v, dict) for v in value):
        raise RuleError(f"{key} holds a non-table")
    return value


def _region(spec):
    spec = spec.strip()
    simple = {"whole_recent", "after_last_prompt_marker", "before_current_prompt_marker",
              "whole_recent_without_current_prompt_marker", "current_prompt_block_marker",
              "after_current_prompt_block_marker", "prompt_box_body", "above_prompt_box",
              "last_non_empty_above_prompt_box", "after_last_horizontal_rule", "osc_title", "osc_progress"}
    if spec in simple:
        return (spec, None)
    m = re.fullmatch(r"(bottom_lines|bottom_non_empty_lines)\((\+?[0-9]+)\)", spec)
    if m:  # Rust's usize parse: digits, a leading + allowed
        return (m.group(1), int(m.group(2)))
    m = re.fullmatch(r"top_non_empty_lines\(([1-9][0-9]*)\)", spec)  # herdr: no leading zero, <= u16::MAX
    if m and int(m.group(1)) <= 0xFFFF:
        return ("top_non_empty_lines", int(m.group(1)))
    return None


def _gate(table, negated):
    def nested(key, neg):
        out = []
        for t in _tables(table.get(key), key):
            _known(t, ("all", "any", "not", "contains", "regex", "line_regex"), "gate")
            out.append(_gate(t, neg))
        return out

    def regexes(key):
        return [_pattern(p) for p in (_strings(table[key], key) if key in table else [])]

    g = Gate(all=nested("all", False), any=nested("any", False), not_=nested("not", True),
             contains=[s.lower() for s in (_strings(table["contains"], "contains") if "contains" in table else [])],
             regex=regexes("regex"), line_regex=regexes("line_regex"))
    empty = not g.positive() and not g.not_
    if (not negated and not g.positive()) or empty:
        raise RuleError("a gate has no matcher")
    return g


def parse_rules(text):
    """One rule file, checked as herdr checks it (deny_unknown_fields, validate_manifest): unknown
    keys, a rule with no positive matcher, an empty `not`, a bad region or regex, and
    `skip_state_update` on a rule that is not `unknown` are errors (RuleError), and so is a file
    that needs a newer engine than ENGINE_VERSION."""
    try:
        doc = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise RuleError(str(error))
    _known(doc, ("id", "version", "min_engine_version", "updated_at", "aliases", "rules"), "file")
    rid = doc.get("id")
    if not isinstance(rid, str):
        raise RuleError("no id")
    version = doc.get("version")
    if version is not None and not isinstance(version, str):
        raise RuleError("version is not a string")
    engine = doc.get("min_engine_version")
    if engine is not None and (type(engine) is not int or not 0 <= engine <= 0xFFFFFFFF):
        raise RuleError("min_engine_version is not a number")
    if engine is not None and engine > ENGINE_VERSION:
        raise RuleError(f"needs engine {engine}, this is {ENGINE_VERSION}")
    aliases = _strings(doc["aliases"], "aliases") if "aliases" in doc else []
    rules = []
    for table in _tables(doc.get("rules"), "rules"):
        rule_id = table.get("id")
        if not isinstance(rule_id, str) or not rule_id.strip():
            raise RuleError("a rule has no id")
        try:
            _known(table, ("id", "state", "priority", "region", "visible_idle", "visible_blocker", "visible_working",
                           "skip_state_update", "all", "any", "not", "contains", "regex", "line_regex"), "rule")
            state = table.get("state", UNKNOWN)
            if state not in STATES:
                raise RuleError(f"bad state {state!r}")

            def flag(key):
                v = table.get(key, False)
                if not isinstance(v, bool):
                    raise RuleError(f"{key} is not a boolean")
                return v

            skip = flag("skip_state_update")
            visible = flag("visible_idle") or flag("visible_blocker") or flag("visible_working")
            if skip and (state != UNKNOWN or "state" not in table or visible):
                raise RuleError('skip_state_update needs state = "unknown" and no visible_* flag')
            priority = table.get("priority", 0)
            if type(priority) is not int:
                raise RuleError("priority is not a number")
            spec = table.get("region", "whole_recent")
            if not isinstance(spec, str):
                raise RuleError("region is not a string")
            region = _region(spec)
            if region is None:
                raise RuleError(f"bad region {spec!r}")
            if region[0] == "top_non_empty_lines" and engine is not None and engine < 3:
                raise RuleError("top_non_empty_lines needs min_engine_version 3")
            gate = _gate(table, False)
        except RuleError as error:
            raise RuleError(f"rule {rule_id}: {error}")
        rules.append(Rule(rule_id, state, priority, region, skip, gate))
    if not rules:
        raise RuleError("no rules")
    return Rules(rid, version, engine, aliases, rules)


@cache
def rules(agent):
    """The rules of agent `agent`, parsed on first use; None for an agent herdr has no file for,
    or whose file does not parse here."""
    name = RULE_FILES.get(agent)
    if name is None:
        return None
    try:
        return parse_rules((RULES_DIR / name).read_text())
    except (OSError, RuleError):
        return None


@cache
def _names():
    names = []
    for agent, name in RULE_FILES.items():
        try:
            doc = tomllib.loads((RULES_DIR / name).read_text())
        except (OSError, tomllib.TOMLDecodeError):
            continue
        names.append((agent, agent))
        aliases = doc.get("aliases") if isinstance(doc.get("aliases"), list) else []
        names.extend((a, agent) for a in aliases if isinstance(a, str) and " " not in a and ":" not in a)
    return names


def agent_of_command(command):
    """The agent a pane's foreground command is (`gemini`, `kiro-cli`), by the ids and aliases in
    the rule files; None for anything else."""
    return next((agent for name, agent in _names() if name == command), None)


def all_process_names():
    """Every command name agent_of_command knows, for the host-side listing script."""
    return [name for name, _ in _names()]


def detect(agent, lines, title):
    """Agent `agent`'s state on a pane whose capture is `lines` and title is `title`; None when
    herdr has no rules for it."""
    r = rules(agent)
    return None if r is None else r.detect(screen_text(lines), title)


def screen_text(lines):
    """A capture as herdr's detection text: each line without its trailing blanks, no blank lines
    at the end, joined by newlines with one after the last."""
    rows = [line.rstrip() for line in lines]
    while rows and not rows[-1].strip():
        rows.pop()
    return "\n".join(rows) + "\n" if rows else ""


# The regions below are herdr's, function for function.

def region_of(region, screen, title):
    kind, n = region
    if kind == "osc_title":
        return title
    if kind == "osc_progress":
        return ""
    if kind == "whole_recent":
        return screen
    if kind == "after_last_prompt_marker":
        return _after_last_prompt_marker(screen)
    if kind == "before_current_prompt_marker":
        return _before_current_prompt_marker(screen)
    if kind == "whole_recent_without_current_prompt_marker":
        return "" if _current_prompt_index(_lines(screen)) is not None else screen
    if kind == "current_prompt_block_marker":
        return _current_prompt_block_marker(screen) or ""
    if kind == "after_current_prompt_block_marker":
        return _after_current_prompt_block_marker(screen) or ""
    if kind == "prompt_box_body":
        return _prompt_box_body(screen) or ""
    if kind == "above_prompt_box":
        return _above_prompt_box(screen)
    if kind == "last_non_empty_above_prompt_box":
        return next((line for line in reversed(_lines(_above_prompt_box(screen))) if line.strip()), "")
    if kind == "after_last_horizontal_rule":
        return _after_last_horizontal_rule(screen)
    lines = _lines(screen)
    if kind == "bottom_lines":
        return _from_line(screen, lines, max(0, len(lines) - n))
    if kind == "bottom_non_empty_lines":
        idx = [i for i, line in enumerate(lines) if line.strip()][-n:] if n else []
        return _from_line(screen, lines, idx[0]) if idx else ""
    if kind == "top_non_empty_lines":
        idx = [i for i, line in enumerate(lines) if line.strip()][:n]
        return screen[:_line_start(screen, lines, idx[-1] + 1)] if idx else ""
    raise AssertionError(kind)


def _after_last_prompt_marker(content):
    lines = _lines(content)
    i = next((i for i in reversed(range(len(lines))) if _prompt_line(lines[i])), None)
    return content if i is None else _from_line(content, lines, i + 1)


def _before_current_prompt_marker(content):
    lines = _lines(content)
    i = _current_prompt_index(lines)
    if i is None:
        return content
    return content[:min(sum(len(line) + 1 for line in lines[:i]), len(content))]


def _current_prompt_block_marker(content):
    lines = _lines(content)
    prompt = _current_prompt_index(lines)
    if prompt is None:
        return None
    return next((line for line in reversed(lines[:prompt]) if _block_marker_line(line)), None)


def _after_current_prompt_block_marker(content):
    lines = _lines(content)
    prompt = _current_prompt_index(lines)
    if prompt is None:
        return None
    block = next((i for i in reversed(range(prompt)) if _block_marker_line(lines[i])), None)
    return None if block is None else _from_line(content, lines, block)


def _current_prompt_index(lines):
    """Codex's last `›` prompt line, unless a response block starts after it."""
    prompt = next((i for i in reversed(range(len(lines))) if _prompt_line(lines[i])), None)
    if prompt is None or any(_block_marker_line(line) for line in lines[prompt + 1:]):
        return None
    return prompt


def _prompt_line(line):
    return line == "›" or line.startswith("› ")


def _block_marker_line(line):
    return line.startswith(("•", "■", "✗", "✓"))


def _prompt_box_body(content):
    lines = _lines(content)
    top = _prompt_box_top(lines)
    if top is None:
        return None
    start = _line_start(content, lines, top + 1)
    end = next((top + 1 + i for i, line in enumerate(lines[top + 1:]) if _horizontal_rule(line)), len(lines))
    end = _line_start(content, lines, end)
    return content[min(start, len(content)):min(end, len(content))]


def _above_prompt_box(content):
    lines = _lines(content)
    top = _prompt_box_top(lines)
    return content if top is None else content[:min(_line_start(content, lines, top), len(content))]


def _after_last_horizontal_rule(content):
    last_end = offset = 0
    for line in _lines(content):
        nxt = offset + len(line) + 1
        if _horizontal_rule(line):
            last_end = min(nxt, len(content))
        offset = nxt
    return content[last_end:]


def _prompt_box_top(lines):
    """The second horizontal rule from the bottom: the top border of a prompt box."""
    rules_at = [i for i in reversed(range(len(lines))) if _horizontal_rule(lines[i])]
    return rules_at[1] if len(rules_at) > 1 else None


def _horizontal_rule(line):
    """A line of `─`, or one that starts with at least three and has a label after them."""
    trimmed = line.strip()
    count = len(trimmed) - len(trimmed.lstrip("─"))
    if count == 0:
        return False
    return not trimmed[count:].lstrip() or count >= 3


def _from_line(content, lines, index):
    return content[min(_line_start(content, lines, index), len(content)):]


def _line_start(content, lines, index):
    return min(sum(len(line) + 1 for line in lines[:min(index, len(lines))]), len(content))
