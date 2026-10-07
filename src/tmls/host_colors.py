"""Stable host accent colors, with optional local overrides."""

import hashlib
import json
from pathlib import Path

from tmls import hosts

CONFIG = Path.home() / ".config" / "tmls" / "host-colors.json"
PALETTE = {
    "blue": "#4468b0",
    "teal": "#0b7d91",
    "violet": "#6f50bc",
    "green": "#2e7d5b",
    "amber": "#9c5c20",
    "rose": "#a64f75",
}


def load():
    try:
        data = json.loads(CONFIG.read_text())
    except (OSError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {host: color for host, color in data.items()
            if isinstance(host, str) and host and isinstance(color, str) and color in PALETTE}


def color_for(host, overrides):
    if host in overrides:
        return PALETTE[overrides[host]]
    index = int.from_bytes(hashlib.sha256(host.encode()).digest()[:4], "big") % len(PALETTE)
    return tuple(PALETTE.values())[index]


def key_for(host):
    return hosts.local_name() if host in {hosts.LOCAL, hosts.KITTY} else host
