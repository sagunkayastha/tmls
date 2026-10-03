"""Answer a Claude permission prompt from tmls: read the dialog off the pane, send 1 or Esc.

Claude's dialog is "Do you want to proceed? 1. Yes … N. No"; "1" approves at once and Esc denies
(the number for No varies, Esc doesn't).
"""
import shlex

from tmls import prompts

QUESTION = "Do you want to proceed?"
KEYS = {True: "1", False: "Escape"}


def request(screen):
    """The dialog's lines (tool, description, command) when a permission prompt is up, else None."""
    lines = screen.splitlines()
    ask = [i for i, line in enumerate(lines) if line.strip() == QUESTION]
    if not ask:
        return None
    top = max((i for i, line in enumerate(lines[:ask[-1]]) if line.startswith("──")), default=-1)
    out = []
    for line in lines[top + 1:ask[-1]]:
        line = line.strip()
        if line and not line.startswith(("╌", "Tip:")):
            out.append(line)
    return out


def _capture_script(name, pane=None):
    # pane: Claude's own ("%7"); after a split the window's active pane may be a shell
    return f"tmux capture-pane -p -t {shlex.quote(pane or f'={name}:')}"


async def current(host, name, pane=None):
    code, out = await prompts._run(prompts._run_argv(host, _capture_script(name, pane)))
    return request(out) if code == 0 else None


async def answer(host, name, shown, yes, pane=None):
    """Send the key only if the dialog you saw is still the one up. None when sent, else why not."""
    if await current(host, name, pane) != shown:
        return f"{name} isn't asking that any more"
    t = shlex.quote(pane or f"={name}:")
    code, out = await prompts._run(prompts._run_argv(host, f"tmux send-keys -t {t} {KEYS[yes]}"))
    return None if code == 0 else (out.strip() or f"couldn't answer {name}")
