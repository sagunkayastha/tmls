"""tmux session, window and pane changes, local or over ssh. No Textual: the web server uses these too."""

import asyncio
import shlex

from tmls import create, hosts

SHELLS = {"sh", "bash", "dash", "zsh", "fish", "ksh", "csh", "tcsh"}


class ManageError(Exception):
    pass


async def _run(host, command):
    argv = command if host == hosts.LOCAL else [
        "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, shlex.join(command)]
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
