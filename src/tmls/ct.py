"""ct: claude inside a tmux session named after the folder, so tmls can open it in a tab.

Rejoins the session if it already exists. Inside tmux it's plain `claude`.
"""
import os
import re
import shlex
import subprocess
import sys


def session_name(folder):
    return re.sub(r"[.:]", "_", os.path.basename(folder.rstrip("/")) or "home")


def exists(name):
    return subprocess.run(["tmux", "has-session", "-t", f"={name}"], capture_output=True).returncode == 0


def plan(folder, args, exists=False):
    """The commands to run, in order; the last one takes over this terminal."""
    if os.environ.get("TMUX"):
        return [["claude", *args]]
    name = session_name(folder)
    attach = ["tmux", "attach", "-t", f"={name}"]
    if exists:
        return [attach]
    return [["tmux", "new-session", "-d", "-s", name, "-c", folder],
            ["tmux", "send-keys", "-t", f"={name}:", shlex.join(["claude", *args]), "Enter"],
            attach]


def main():
    folder = os.getcwd()
    steps = plan(folder, sys.argv[1:], exists=not os.environ.get("TMUX") and exists(session_name(folder)))
    for step in steps[:-1]:
        subprocess.run(step, check=True)
    os.execvp(steps[-1][0], steps[-1])
