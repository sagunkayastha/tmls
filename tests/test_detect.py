"""herdr's agent-detection rules and the Python engine that reads them (tmls/detect.py), against
panes of codex 0.161.0 and opencode 1.18.33 captured on archbox on 2026-10-14 (tests/data/
agent-screens: what `tmux capture-pane -p` printed, and the pane's `#{pane_title}`), and against
made-up screens for the engine's corners. Mirrors tmls-rs's crates/client-core/tests/detect.rs."""
import re
from pathlib import Path

import pytest

from tmls import detect
from tmls.detect import BLOCKED, IDLE, UNKNOWN, WORKING, Detection

DATA = Path(__file__).parent / "data" / "agent-screens"


def capture(name):
    return (DATA / f"{name}.screen").read_text(), (DATA / f"{name}.title").read_text().rstrip("\n")


def on(agent, name):
    screen, title = capture(name)
    d = detect.detect(agent, detect._lines(screen), title)
    return d.state, d.rule


def state(agent, screen, title=""):
    return detect.detect(agent, detect._lines(screen), title).state


def toy(text):
    return detect.parse_rules(text)


def test_every_vendored_rule_file_parses_and_is_listed_in_the_index():
    index = (detect.RULES_DIR / "index.toml").read_text()
    for agent, name in detect.RULE_FILES.items():
        parsed = detect.parse_rules((detect.RULES_DIR / name).read_text())
        assert parsed.id == agent
        assert parsed.min_engine_version is not None and parsed.min_engine_version <= detect.ENGINE_VERSION
        assert detect.rules(agent) is not None, agent
        assert f'id = "{agent}"' in index and f'path = "{name}"' in index, agent
    assert index.count("[[agents]]") == len(detect.RULE_FILES), "index.toml lists a file that is not vendored"
    assert detect.rules("mastracode") is None, "herdr has no screen rules for it"
    assert len(detect.HERDR_COMMIT) == 40
    assert detect.HERDR_COMMIT in (detect.RULES_DIR / "NOTICE").read_text()
    assert "Apache License" in (detect.RULES_DIR / "LICENSE").read_text()


def test_codex_captures():
    # The title alone says it: a spinner while it works, "Action Required" while it asks.
    assert on("codex", "codex-start") == (IDLE, "osc_title_idle")
    assert on("codex", "codex-working") == (WORKING, "osc_title_working")
    assert on("codex", "codex-idle-after") == (IDLE, "osc_title_idle")
    assert on("codex", "codex-approval-blocked") == (BLOCKED, "osc_title_blocked")
    # The update picker at start-up, before codex has set a title: tmux shows the host's name.
    assert on("codex", "codex-update-blocked") == (BLOCKED, "startup_update")


@pytest.mark.parametrize("title", ["", "sgkayast@archbox:~/play"])
def test_codex_captures_read_by_their_screen_alone(title):
    # A title from before (the shell's, or none) still leaves the screen rules to decide.
    def by_screen(name):
        screen, _ = capture(name)
        d = detect.detect("codex", detect._lines(screen), title)
        return d.state, d.rule

    assert by_screen("codex-working") == (WORKING, "screen_working_fallback")
    assert by_screen("codex-approval-blocked") == (BLOCKED, "live_strong_blocker")
    assert by_screen("codex-idle-after")[0] == IDLE
    assert by_screen("codex-start")[0] == IDLE


def test_opencode_captures():
    assert on("opencode", "opencode-idle") == (IDLE, None)
    assert on("opencode", "opencode-idle-after") == (IDLE, None)
    assert on("opencode", "opencode-working") == (WORKING, "progress_bar_working")
    assert on("opencode", "opencode-permission-blocked") == (BLOCKED, "permission_required")


def test_highest_priority_wins_and_a_tie_goes_to_the_first_rule():
    t = toy('''
id = "toy"
[[rules]]
id = "first"
state = "working"
priority = 10
contains = ["busy"]
[[rules]]
id = "second"
state = "blocked"
priority = 10
contains = ["busy"]
[[rules]]
id = "louder"
state = "blocked"
priority = 20
contains = ["ask"]
''')
    assert t.detect("busy\n", "").rule == "first"
    assert t.detect("BUSY, then ask\n", "").rule == "louder", "contains ignores case"
    assert t.detect("nothing\n", "") == Detection(IDLE, None, False)


def test_nested_gates_regions_and_skip_state_update():
    t = toy(r'''
id = "toy"
min_engine_version = 3
[[rules]]
id = "viewer"
state = "unknown"
priority = 50
skip_state_update = true
region = "bottom_non_empty_lines(1)"
contains = ["q to quit"]
[[rules]]
id = "top"
state = "blocked"
priority = 40
region = "top_non_empty_lines(1)"
regex = ['\ATrust\?']
[[rules]]
id = "nest"
state = "working"
priority = 30
any = [{ contains = ["spin"] }, { all = [{ line_regex = ['^\d+s$'] }] }]
not = [{ contains = ["stopped"] }]
[[rules]]
id = "title"
state = "working"
priority = 5
region = "osc_title"
contains = ["⠋"]
''')
    d = t.detect("history\nq to quit\n", "")
    assert (d.state, d.skip_state_update) == (UNKNOWN, True)
    assert t.detect("q to quit\nmore\n", "").rule is None, "only the bottom line counts"
    assert t.detect("Trust?\nyes\n", "").rule == "top"
    # herdr counts non-empty lines but keeps the blank ones above them in the region: \A sees those
    assert t.detect("\n\nTrust?\nyes\n", "").rule is None
    assert t.detect("x\nTrust?\n", "").rule is None
    assert t.detect("spin\n", "").rule == "nest"
    assert t.detect("work\n12s\n", "").rule == "nest"
    assert t.detect("spin\nstopped\n", "").rule is None
    assert t.detect("idle\n", "⠋ codex").rule == "title"
    assert t.detect("idle\n⠋\n", "").rule is None, "the title rule reads the title only"


@pytest.mark.parametrize("text,why", [
    ('id = "x"\n', "no rules"),
    ('id = "x"\nwho = 1\n[[rules]]\nid = "r"\ncontains = ["a"]\n', "unknown"),
    ('id = "x"\n[[rules]]\nid = "r"\ncolour = 1\ncontains = ["a"]\n', "unknown"),
    ('id = "x"\n[[rules]]\nid = "r"\nstate = "idle"\n', "matcher"),
    ('id = "x"\n[[rules]]\nid = "r"\nregion = "middle"\ncontains = ["a"]\n', "region"),
    ('id = "x"\n[[rules]]\nid = "r"\nregion = "top_non_empty_lines(05)"\ncontains = ["a"]\n', "region"),
    ('id = "x"\n[[rules]]\nid = "r"\nregex = [\'(\']\n', "regex"),
    ('id = "x"\n[[rules]]\nid = "r"\nstate = "idle"\nskip_state_update = true\ncontains = ["a"]\n', "skip_state_update"),
    ('id = "x"\n[[rules]]\nid = "r"\ncontains = ["a"]\nnot = [{}]\n', "matcher"),
    ('id = "x"\nmin_engine_version = 4\n[[rules]]\nid = "r"\ncontains = ["a"]\n', "engine"),
    ('id = "x"\nmin_engine_version = 2\n[[rules]]\nid = "r"\nregion = "top_non_empty_lines(3)"\ncontains = ["a"]\n',
     "top_non_empty_lines"),
    ('id = "x"\n[[rules]]\nid = " "\ncontains = ["a"]\n', "id"),
    ("not toml [", ""),
])
def test_bad_rule_files_are_refused_as_herdr_refuses_them(text, why):
    with pytest.raises(detect.RuleError, match=re.escape(why) if why else None):
        detect.parse_rules(text)


def test_a_capture_becomes_herdrs_detection_text():
    assert detect.screen_text(["a  ", "", "b", "   ", ""]) == "a\n\nb\n"
    assert detect.screen_text(["", "  "]) == ""
    assert detect.screen_text([]) == ""


def test_process_names_come_from_the_rule_files():
    for command, agent in [("codex", "codex"), ("opencode", "opencode"), ("open-code", "opencode"),
                           ("gemini", "gemini"), ("kiro-cli", "kiro"), ("cursor-agent", "cursor"),
                           ("ghcs", "copilot"), ("claude", "claude")]:
        assert detect.agent_of_command(command) == agent, command
    for command in ["bash", "node", "vim", "herdr:opencode", "kimi code", ""]:
        assert detect.agent_of_command(command) is None, command
    # Rules for an agent not captured here (gemini runs under node), straight from its file.
    assert state("gemini", "╭───\n│ Allow execution of: 'rm'?\n╰───\n") == BLOCKED
    assert state("gemini", "⠼ Thinking (esc to cancel, 3s)\n") == WORKING
    assert state("gemini", "> Type your message\n") == IDLE


def test_rust_regex_spellings_read_the_same_in_python():
    # \x{...}, \z and a $ that is the end of the text only (Python's $ also matches before a last \n)
    assert detect._pattern(r"[\x{2801}-\x{28FF}]").search("⠋")
    assert detect._pattern(r"done\z").search("done") and not detect._pattern(r"done\z").search("done\n")
    assert not detect._pattern(r"done$").search("done\n")
    assert detect._pattern(r"(?m)^done$").search("done\nmore")
    assert detect._pattern(r"[$]").search("$") and detect._pattern(r"\$").search("$")
    assert detect._pattern(r"\p{Alphabetic}+ing").search("Thinking")
    with pytest.raises(detect.RuleError):
        detect._pattern(r"[\p{Alphabetic}]")
