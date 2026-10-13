"""Claude Code's own record of a conversation, read as chat items. Lines in, items out: no host here.

The record (~/.claude/projects/<slug>/<sessionId>.jsonl) has one JSON object per line. Only the
entries of the conversation chain carry a "uuid": what you typed, Claude's replies (one content
block per entry: text, thinking or a tool call), tool results (as "user" entries), and what the
harness slipped in. The rest (titles, modes, snapshots) is bookkeeping without one.
"""
import base64
import binascii
import json
import re
from datetime import datetime
from pathlib import PurePosixPath

HISTORY = 300        # items sent when a chat opens
INPUT_MAX = 4000     # characters of a tool call's input
OUTPUT_MAX = 8000    # characters of its result
SUMMARY_MAX = 80
# Served back with this Content-Type from the site's own origin, so never image/svg+xml.
IMAGE_TYPES = {"image/png", "image/jpeg", "image/gif", "image/webp"}
# A "user" entry that starts like this was written by the harness, not typed.
HARNESS = ("<task-notification", "<system-reminder", "<local-command-", "<bash-stdout", "<bash-stderr",
           "<cross-session-message", "<user-prompt-submit-hook", "<command-message", "<command-name")
INTERRUPTED = "[Request interrupted by user"
SUMMARIES = {  # tool -> (verb, input field); a path shows as its file name, a pattern in quotes
    "Bash": ("Ran", "command"), "Read": ("Read", "file_path"), "Edit": ("Edited", "file_path"),
    "MultiEdit": ("Edited", "file_path"), "NotebookEdit": ("Edited", "notebook_path"),
    "Write": ("Wrote", "file_path"), "Grep": ("Searched", "pattern"), "Glob": ("Searched", "pattern"),
    "WebSearch": ("Searched", "query"), "ToolSearch": ("Searched tools", "query"),
    "WebFetch": ("Fetched", "url"), "Agent": ("Agent:", "description"), "Task": ("Agent:", "description"),
    "Skill": ("Skill", "skill"),
}
QUOTED = {"pattern", "query"}
PATHS = {"file_path", "notebook_path"}


def clip(text, limit):
    return text if len(text) <= limit else text[:limit - 1] + "…"


def epoch(stamp):
    """"2026-10-13T08:15:00.123Z" as epoch seconds; 0 when the entry has no usable time."""
    try:
        return int(datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp())
    except (AttributeError, ValueError):
        return 0


def typed(text):
    """What the user typed, out of a user entry's text; None when the harness wrote all of it.
    A slash command is stored wrapped (<command-name>/clear</command-name>…) and `!cmd` as
    <bash-input>: those come back as typed, without the wrapper."""
    text = re.sub(r"<system-reminder>.*?</system-reminder>", "", text, flags=re.S).strip()
    command = re.search(r"<command-name>(.*?)</command-name>", text, re.S)
    if command and text.startswith(("<command-name>", "<command-message>")):
        args = re.search(r"<command-args>(.*?)</command-args>", text, re.S)
        return f"{command.group(1).strip()} {args.group(1).strip() if args else ''}".strip() or None
    shell = re.fullmatch(r"<bash-input>(.*)</bash-input>", text, re.S)
    if shell:
        return "! " + shell.group(1).strip()
    if text.startswith(HARNESS) or text.startswith(INTERRUPTED):
        return None
    return text or None


def summary(tool, given):
    """One short line for a folded tool row: `Ran ls -la`, `Read app.rs`, `Searched "foo"`."""
    given = given if isinstance(given, dict) else {}
    verb, field = SUMMARIES.get(tool, (None, None))
    if verb is None:  # a tool this reader has no words for: its name and its first short argument
        verb = tool.split("__")[-1]
        field = next((k for k, v in given.items() if isinstance(v, str) and v.strip()), None)
    value = given.get(field)
    value = " ".join(value.split()) if isinstance(value, str) else ""
    if field in PATHS:
        value = PurePosixPath(value).name or value
    elif field in QUOTED and value:
        value = f'"{value}"'
    return clip(f"{verb} {value}".strip(), SUMMARY_MAX)


def readable(tool, given):
    """A tool call's whole input as text a person can read: a command as it is, else `field: value`."""
    if isinstance(given, str):
        return clip(given, INPUT_MAX)
    if not isinstance(given, dict):
        return clip(json.dumps(given, ensure_ascii=False), INPUT_MAX)
    if tool == "Bash" and isinstance(given.get("command"), str):
        return clip(given["command"], INPUT_MAX)
    lines = [f"{k}: {v if isinstance(v, str) else json.dumps(v, ensure_ascii=False)}" for k, v in given.items()]
    return clip("\n".join(lines), INPUT_MAX)


def _image(block, blob_id, blobs):
    """A base64 image block as a BLOB ({"id", "mime"}), its bytes appended to blobs; None if it isn't one."""
    source = block.get("source")
    if not isinstance(source, dict) or source.get("type") != "base64" or source.get("media_type") not in IMAGE_TYPES:
        return None
    try:
        data = base64.b64decode(source.get("data") or "")
    except (binascii.Error, ValueError, TypeError):
        return None
    if not data:
        return None
    blobs.append((blob_id, source["media_type"], data))
    return {"id": blob_id, "mime": source["media_type"]}


def _result(block, blobs):
    """A tool_result block as the patch for its tool item (the tool_use block's id names it)."""
    tool_id, content = block.get("tool_use_id"), block.get("content")
    if not isinstance(tool_id, str):
        return None
    texts, images = [], []
    if isinstance(content, str):
        texts.append(content)
    for part in content if isinstance(content, list) else []:
        if not isinstance(part, dict):
            continue
        if part.get("type") == "text" and isinstance(part.get("text"), str):
            texts.append(part["text"])
        elif part.get("type") == "image":
            image = _image(part, f"{tool_id}-{len(images)}", blobs)
            if image:
                images.append(image)
    return {"t": "patch", "id": tool_id, "output": clip("\n".join(texts), OUTPUT_MAX),
            "error": bool(block.get("is_error")), "images": images}


def _assistant(entry, base):
    message = entry.get("message") if isinstance(entry.get("message"), dict) else {}
    content = message.get("content")
    if isinstance(content, str):
        content = [{"type": "text", "text": content}]
    out = []
    for i, block in enumerate(b for b in content or [] if isinstance(b, dict)):
        item = {**base, "id": base["id"] if i == 0 else f"{base['id']}:{i}"}
        kind = block.get("type")
        if kind == "text":
            text = block.get("text") if isinstance(block.get("text"), str) else ""
            if text.strip():  # an API error is Claude Code talking, not Claude
                out.append({**item, "kind": "note" if entry.get("isApiErrorMessage") else "assistant", "text": text})
        elif kind == "tool_use":
            tool = block.get("name") if isinstance(block.get("name"), str) else "tool"
            out.append({**item, "id": block["id"] if isinstance(block.get("id"), str) else item["id"],
                        "kind": "tool", "tool": tool, "summary": summary(tool, block.get("input")),
                        "input": readable(tool, block.get("input")), "output": None, "error": False, "images": []})
        elif kind not in ("thinking", "redacted_thinking"):
            out.append({**item, "kind": "note", "text": f"Unknown content ({kind})"})
    return out


def _user(entry, base, blobs):
    message = entry.get("message") if isinstance(entry.get("message"), dict) else {}
    content = message.get("content")
    origin = entry.get("origin")
    # origin says who wrote it (older records have none): a sub-agent's or a task's message isn't yours
    human = not isinstance(origin, dict) or origin.get("kind") in (None, "human")
    if isinstance(content, str):
        content = [{"type": "text", "text": content}]
    out, texts, images = [], [], []
    for i, block in enumerate(b for b in content or [] if isinstance(b, dict)):
        kind = block.get("type")
        if kind == "tool_result":
            patch = _result(block, blobs)
            if patch:
                out.append(patch)
        elif not human or entry.get("isCompactSummary") or entry.get("isVisibleInTranscriptOnly"):
            continue
        elif kind == "text" and isinstance(block.get("text"), str):
            if block["text"].startswith(INTERRUPTED):
                out.append({**base, "id": f"{base['id']}:{i}", "kind": "note", "text": "Interrupted"})
            elif typed(block["text"]):
                texts.append(typed(block["text"]))
        elif kind == "image":
            image = _image(block, f"{base['id']}-{len(images)}", blobs)
            if image:
                images.append(image)
        elif kind not in ("text", "document"):
            out.append({**base, "id": f"{base['id']}:{i}", "kind": "note", "text": f"Unknown content ({kind})"})
    if texts or images:
        out.append({**base, "kind": "user", "text": "\n\n".join(texts), "images": images})
    return out


def read(line):
    """One record line as (events, blobs). An event is an ITEM (it has "kind") or a
    {"t": "patch"} carrying a tool call's result; blobs are (id, mime, bytes) of the images in it.
    Nothing for thinking, sub-agents, meta entries, bookkeeping and what the harness injected."""
    try:
        entry = json.loads(line)
    except ValueError:  # a line caught mid-write, or the cut-off start of a tail
        return [], []
    if not isinstance(entry, dict) or not isinstance(entry.get("uuid"), str):
        return [], []  # bookkeeping: titles, modes, file snapshots
    if entry.get("isSidechain") or entry.get("isMeta"):
        return [], []
    base, kind, blobs = {"id": entry["uuid"], "ts": epoch(entry.get("timestamp"))}, entry.get("type"), []
    if kind == "assistant":
        return _assistant(entry, base), blobs
    if kind == "user":
        return _user(entry, base, blobs), blobs
    if kind == "attachment":
        # typed while Claude was busy: delivered later, and recorded only as this attachment
        a = entry.get("attachment") if isinstance(entry.get("attachment"), dict) else {}
        origin = a.get("origin") if isinstance(a.get("origin"), dict) else {}
        if (a.get("type") == "queued_command" and a.get("commandMode") == "prompt" and not a.get("isMeta")
                and origin.get("kind") in (None, "human") and isinstance(a.get("prompt"), str) and typed(a["prompt"])):
            return [{**base, "kind": "user", "text": typed(a["prompt"]), "images": []}], blobs
        return [], []
    if kind == "system":
        if entry.get("subtype") == "compact_boundary":
            return [{**base, "kind": "note", "text": "Context compacted"}], blobs
        return [], []  # turn durations, hook summaries, local command output
    return [{**base, "kind": "note", "text": f"Unknown entry ({kind})"}], blobs


def fold(events, limit=HISTORY):
    """History as one list: each result put into its tool item, then the newest `limit` items.
    A result whose call is not among the events (it is before the part that was read) is dropped."""
    items, tools = [], {}
    for event in events:
        if "kind" in event:
            items.append(event)
            if event["kind"] == "tool":
                tools[event["id"]] = event
        elif event["id"] in tools:
            tools[event["id"]].update(output=event["output"], error=event["error"], images=event["images"])
    return items[-limit:]
