"""Answer a Claude permission prompt from tmls: read the dialog off the pane, send 1 or Esc.

Claude's dialog is "Do you want to proceed? 1. Yes … N. No" (for a file: "Do you want to create
hello.txt?", "… make this edit to x.py?"); "1" approves at once and Esc denies
(the number for No varies, Esc doesn't).
"""
import re
import shlex

from tmls import prompts

QUESTION = re.compile(r"Do you want to .+\?")
FIRST = re.compile(r"(❯\s*)?1\. Yes")  # the first choice, under the question: text in a reply has none
CHOICE = re.compile(r"(?:❯\s*)?(\d+)\.\s+(.+)")
KEYS = {True: "1", False: "Escape"}


def request(screen):
    """The dialog's lines (tool, description, command) when a permission prompt is up, else None."""
    lines = screen.splitlines()
    ask = [i for i, line in enumerate(lines) if QUESTION.fullmatch(line.strip())
           and any(FIRST.match(after.strip()) for after in lines[i + 1:i + 3])]
    if not ask:
        return None
    top = max((i for i, line in enumerate(lines[:ask[-1]]) if line.startswith("──")), default=-1)
    out = []
    for line in lines[top + 1:ask[-1]]:
        line = line.strip()
        if line and not line.startswith(("╌", "Tip:")):
            out.append(line)
    return out


def choices(screen):
    """The numbered choices under the question ([{"n": 1, "label": "Yes"}, ...]) when a permission
    prompt is up, else None. The numbers aren't always consecutive."""
    lines = screen.splitlines()
    ask = [i for i, line in enumerate(lines) if QUESTION.fullmatch(line.strip())
           and any(FIRST.match(after.strip()) for after in lines[i + 1:i + 3])]
    if not ask:
        return None
    out = []
    for line in lines[ask[-1] + 1:]:
        line = line.strip()
        m = CHOICE.fullmatch(line)
        if m:
            out.append({"n": int(m.group(1)), "label": m.group(2).strip()})
        elif line.startswith(("Esc to cancel", "──")):
            break
        elif line and out:  # a label that wrapped onto the next line
            out[-1]["label"] += " " + line
    return out


def _capture_script(name, pane=None):
    # pane: Claude's own ("%7"); after a split the window's active pane may be a shell
    return f"tmux capture-pane -p -t {shlex.quote(pane or f'={name}:')}"


async def current(host, name, pane=None):
    code, out = await prompts._run(prompts._run_argv(host, _capture_script(name, pane)))
    return request(out) if code == 0 else None


async def current_choices(host, name, pane=None):
    code, out = await prompts._run(prompts._run_argv(host, _capture_script(name, pane)))
    return choices(out) if code == 0 else None


async def answer(host, name, shown, yes, pane=None, choice=None):
    """Send the key only if the dialog you saw is still the one up. None when sent, else why not.
    choice: the number of a choice on screen, sent instead of 1/Escape."""
    code, screen = await prompts._run(prompts._run_argv(host, _capture_script(name, pane)))
    if code != 0 or request(screen) != shown:
        return f"{name} isn't asking that any more"
    if choice is not None and choice not in [c["n"] for c in choices(screen)]:
        return f"{name} isn't offering choice {choice} any more"
    key = str(choice) if choice is not None else KEYS[yes]
    t = shlex.quote(pane or f"={name}:")
    code, out = await prompts._run(prompts._run_argv(host, f"tmux send-keys -t {t} {key}"))
    return None if code == 0 else (out.strip() or f"couldn't answer {name}")
