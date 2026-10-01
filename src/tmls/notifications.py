"""Saved alert switches and desktop notification commands."""

import asyncio
import json
from dataclasses import asdict, dataclass
from pathlib import Path

CONFIG = Path.home() / ".config" / "tmls" / "notifications.json"
SYMBOL = {"waiting": "?", "done": "◆"}


@dataclass
class Settings:
    desktop: bool = True
    sound: bool = False
    silence_focused: bool = True


def load():
    try:
        data = json.loads(CONFIG.read_text())
    except (OSError, ValueError):
        return Settings()
    defaults = Settings()
    return Settings(**{key: data.get(key) if isinstance(data.get(key), bool) else getattr(defaults, key)
                       for key in vars(defaults)}) if isinstance(data, dict) else defaults


def save(settings):
    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    CONFIG.write_text(json.dumps(asdict(settings)) + "\n")


async def _run(*argv):
    try:
        process = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
        await asyncio.wait_for(process.wait(), timeout=5)
    except asyncio.TimeoutError:
        process.kill()
        await process.wait()
    except OSError:
        pass  # no desktop notifier or sound player on this machine


async def emit(settings, host, name, mark, reason):
    if mark not in SYMBOL:
        return
    detail = reason if mark == "waiting" and reason else ("waiting" if mark == "waiting" else "done")
    if settings.desktop:
        await _run("notify-send", f"tmls · {host}", f"{SYMBOL[mark]} {name} · {detail}")
    if settings.sound:
        await _run("canberra-gtk-play", "-i", "message-new-instant", "-d", "tmls")
