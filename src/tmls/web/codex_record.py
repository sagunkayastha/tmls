"""Codex's own record of a conversation, read as chat items: the same items record.py makes for Claude.

The record (~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl) has one JSON object per
line. Codex writes each finished turn item as an `event_msg` of type `item_completed`: what you
typed, its replies, the commands it ran, file changes, searches. Those are what the chat shows;
the raw model traffic (`response_item`), reasoning and token counts are left out.
"""
import json
from pathlib import PurePosixPath

from tmls.web.record import HISTORY, INPUT_MAX, OUTPUT_MAX, SUMMARY_MAX, clip, epoch


def _text(content, kind):
    return "\n\n".join(c["text"] for c in content or [] if isinstance(c, dict) and c.get("type") == kind
                       and isinstance(c.get("text"), str) and c["text"].strip())


def _name(path):
    path = path.removeprefix("file://") if isinstance(path, str) else ""
    return PurePosixPath(path).name or path


def _command(item):
    """The command as typed: `bash -lc "…"` is shown as the "…"."""
    argv = item.get("command")
    if isinstance(argv, list) and all(isinstance(a, str) for a in argv):
        if len(argv) == 3 and argv[1] in ("-lc", "-c"):
            return argv[2]
        return " ".join(argv)
    return argv if isinstance(argv, str) else ""


def _tool(base, tool, summary, given, output, error=False):
    return {**base, "kind": "tool", "tool": tool, "summary": clip(" ".join(summary.split()), SUMMARY_MAX),
            "input": clip(given, INPUT_MAX), "output": clip(output, OUTPUT_MAX), "error": error, "images": []}


def _item(item, base):
    kind = item.get("type")
    if kind == "UserMessage":
        text = _text(item.get("content"), "text")
        return [{**base, "kind": "user", "text": text, "images": []}] if text else []
    if kind == "AgentMessage":
        text = _text(item.get("content"), "Text")
        return [{**base, "kind": "assistant", "text": text}] if text else []
    if kind == "CommandExecution":
        command = _command(item)
        code = item.get("exit_code")
        return [_tool(base, "Bash", f"Ran {command}", command, item.get("aggregated_output") or "",
                      error=isinstance(code, int) and code != 0)]
    if kind == "FileChange":
        changes = item.get("changes") if isinstance(item.get("changes"), dict) else {}
        names = [_name(p) for p in changes]
        given = "\n".join(f"{(c or {}).get('type', 'update')}: {p}" for p, c in changes.items())
        output = "\n".join(s for s in (item.get("stdout"), item.get("stderr")) if isinstance(s, str) and s)
        summary = f"Edited {names[0]}" if len(names) == 1 else f"Edited {len(names)} files"
        return [_tool(base, "Edit", summary, given, output, error=item.get("status") == "failed")]
    if kind == "ImageView":
        return [_tool(base, "Read", f"Viewed {_name(item.get('path'))}", str(item.get("path") or ""), "")]
    if kind == "McpToolCall":
        tool = f"{item.get('server')}.{item.get('tool')}"
        result = item.get("result") if isinstance(item.get("result"), dict) else {}
        output = "\n".join(c.get("text", "") for c in result.get("content") or [] if isinstance(c, dict))
        return [_tool(base, tool, tool, json.dumps(item.get("arguments"), ensure_ascii=False), output,
                      error=item.get("status") == "failed")]
    if kind == "Extension" and item.get("kind") == "web.search":
        query = item.get("query") if isinstance(item.get("query"), str) else ""
        results = [r for r in item.get("results") or [] if isinstance(r, dict)]
        output = "\n".join(f"{r.get('title', '')}\n{r.get('url', '')}" for r in results)
        return [_tool(base, "WebSearch", f'Searched "{query}"', query, output)]
    if kind == "ContextCompaction":
        return [{**base, "kind": "note", "text": "Context compacted"}]
    return []  # reasoning, clock waits, sub-agent bookkeeping


def read(line):
    """One record line as (events, blobs), like record.read. Codex writes an item once it is done,
    so a tool item comes with its result and there are no patches; no images either."""
    try:
        entry = json.loads(line)
    except ValueError:  # a line caught mid-write, or the cut-off start of a tail
        return [], []
    if not isinstance(entry, dict) or entry.get("type") != "event_msg":
        return [], []
    payload = entry.get("payload") if isinstance(entry.get("payload"), dict) else {}
    ts = epoch(entry.get("timestamp"))
    if payload.get("type") == "item_completed" and isinstance(payload.get("item"), dict):
        item = payload["item"]
        if not isinstance(item.get("id"), str):
            return [], []
        return _item(item, {"id": item["id"], "ts": ts}), []
    if payload.get("type") == "turn_aborted" and payload.get("reason") == "interrupted":
        turn = payload.get("turn_id") if isinstance(payload.get("turn_id"), str) else str(ts)
        return [{"id": f"aborted-{turn}", "ts": ts, "kind": "note", "text": "Interrupted"}], []
    return [], []


def fold(events, limit=HISTORY):
    return [e for e in events if "kind" in e][-limit:]
