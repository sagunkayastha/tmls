"""tmux session, window and pane changes, local or over ssh. No Textual: the web server uses these too."""

import asyncio
import json
import re
import shlex

from tmls import create, hosts, prompts

SHELLS = {"sh", "bash", "dash", "zsh", "fish", "ksh", "csh", "tcsh"}


class ManageError(Exception):
    pass


async def _run(host, command):
    argv = command if host == hosts.LOCAL else hosts.run_argv(host, shlex.join(command))
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(proc.communicate(), timeout=8)
    except asyncio.TimeoutError as error:
        proc.kill()
        await proc.wait()
        raise ManageError(f"{hosts.label(host)} timed out") from error
    except OSError as error:
        raise ManageError(str(error)) from error
    if proc.returncode:
        raise ManageError((err or out).decode(errors="replace").strip() or "tmux command failed")
    return out.decode(errors="replace")


async def rename(host, old, new):
    error = create.check_name(new)
    if error:
        return error
    try:
        await _run(host, ["tmux", "rename-session", "-t", "=" + old, "--", new])
    except ManageError as error:
        return str(error)
    return None


CLAUDE_FILES = ('for f in "$HOME"/.claude/sessions/*.json; do p=${f##*/}; '
                'kill -0 "${p%.json}" 2>/dev/null && cat "$f" && echo; done; true')


def claude_in(out, names):
    """The Claude in a session, from "<pane ids>\n---\n<session files>": its file, or None.
    Its "tmux" field ("name:@1.%3") must name one of the panes and one of `names` (the old or
    new session name: the file may not have caught up). Pane ids alone repeat across tmux servers."""
    panes, _, files = out.partition("\n---\n")
    panes = set(panes.split())
    for line in files.splitlines():
        try:
            c = json.loads(line)
        except ValueError:
            continue
        m = re.fullmatch(r"(.*):@\d+\.(%\d+)", c.get("tmux") or "") if isinstance(c, dict) else None
        if m and m.group(2) in panes and m.group(1) in names:
            return {**c, "pane": m.group(2)}
    return None


async def rename_claude(host, old, name, title):
    """Give the Claude in tmux session `name` (just renamed from `old`) the title too, with its
    /rename. None when done or
    there's no Claude; else a note on why it kept its name. Only typed when Claude is idle: busy,
    the text would wait in its input; at a permission prompt, it would answer the prompt."""
    script = (f"tmux list-panes -s -t {shlex.quote('=' + name)} -F '#{{pane_id}}'; echo ---; "
              + CLAUDE_FILES)
    try:
        out = await _run_script(host, script)
    except ManageError as error:
        return f"Claude's name unchanged: {error}"
    claude = claude_in(out, {old, name})
    if claude is None or claude.get("name") == title:
        return None
    status = claude.get("status")
    if status in ("busy", "waiting"):
        why = "working" if status == "busy" else "waiting on you"
        return f"Claude is {why}, so it keeps its name; type /rename {title} there later."
    error = await prompts.send(host, name, f"/rename {title}", pane=claude["pane"])
    return f"Claude's name unchanged: {error}" if error else None


async def _run_script(host, script):
    proc = await asyncio.create_subprocess_exec(
        *hosts.run_argv(host, script), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try:
        out, err = await asyncio.wait_for(proc.communicate(), timeout=8)
    except asyncio.TimeoutError as error:
        proc.kill()
        await proc.wait()
        raise ManageError(f"{hosts.label(host)} timed out") from error
    if proc.returncode:
        raise ManageError((err or out).decode(errors="replace").strip() or "listing failed")
    return out.decode(errors="replace")


async def running_commands(host, name):
    output = await _run(host, ["tmux", "list-panes", "-s", "-t", "=" + name,
                               "-F", "#{pane_current_command}"])
    return sorted({command for command in output.splitlines() if command and command not in SHELLS})


async def kill(host, name):
    try:
        await _run(host, ["tmux", "kill-session", "-t", "=" + name])
    except ManageError as error:
        return str(error)
    return None


async def _change(host, command):
    try:
        await _run(host, command)
    except ManageError as error:
        return str(error)
    return None


async def new_window(host, name):
    return await _change(host, ["tmux", "new-window", "-t", "=" + name + ":",
                                "-c", "#{pane_current_path}"])


async def rename_window(host, name, new_name):
    if not new_name.strip():
        return "the window needs a name"
    return await _change(host, ["tmux", "rename-window", "-t", "=" + name + ":", new_name])


async def split(host, name, direction):
    if direction not in {"h", "v"}:
        raise ValueError(direction)
    return await _change(host, ["tmux", "split-window", "-" + direction,
                                "-t", "=" + name + ":", "-c", "#{pane_current_path}"])


async def pane_info(host, name):
    target = "=" + name + ":"
    command = (await _run(host, ["tmux", "display-message", "-p", "-t", target,
                                 "#{pane_current_command}"])).strip()
    panes = (await _run(host, ["tmux", "list-panes", "-s", "-t", target,
                               "-F", "#{pane_id}"])).splitlines()
    return (command if command not in SHELLS else None), len(panes) == 1


async def kill_pane(host, name):
    return await _change(host, ["tmux", "kill-pane", "-t", "=" + name + ":"])
