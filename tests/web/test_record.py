import base64
import json

from tmls.web import record

PNG = base64.b64encode(b"\x89PNG fake").decode()


def entry(kind, content=None, uuid="u1", **more):
    e = {"type": kind, "uuid": uuid, "timestamp": "2026-10-13T08:15:00.500Z", **more}
    if content is not None:
        e["message"] = {"role": kind, "content": content}
    return json.dumps(e)


def items(line):
    return record.read(line)[0]


def image(mime="image/png", data=PNG):
    return {"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}


def test_what_you_typed_is_a_user_item_with_the_entry_time():
    assert items(entry("user", "fix the parser", origin={"kind": "human"})) == [
        {"id": "u1", "ts": 1791879300, "kind": "user", "text": "fix the parser", "images": []}]
    # older records have no origin; text typed as blocks counts too
    assert items(entry("user", [{"type": "text", "text": "and the tests"}]))[0]["text"] == "and the tests"


def test_harness_text_is_not_yours():
    for text in ("<task-notification>done</task-notification>", "<system-reminder>be good</system-reminder>",
                 "<local-command-stdout>ok</local-command-stdout>", "<local-command-caveat>x</local-command-caveat>",
                 "<bash-stdout>out</bash-stdout>"):
        assert items(entry("user", text)) == []
    assert items(entry("user", "a peer wrote this", origin={"kind": "peer", "from": "x"})) == []
    assert items(entry("user", "meta", isMeta=True)) == []
    assert items(entry("user", "a sub-agent's prompt", isSidechain=True)) == []
    assert items(entry("user", "the summary of all before", isCompactSummary=True)) == []
    # a reminder tacked onto your message goes, the message stays
    mixed = "run it<system-reminder>\nremember\n</system-reminder>"
    assert items(entry("user", mixed))[0]["text"] == "run it"


def test_commands_come_back_as_typed():
    wrapped = "<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>opus</command-args>"
    assert items(entry("user", wrapped))[0]["text"] == "/model opus"
    assert items(entry("user", "<command-message>x</command-message>\n<command-name>/clear</command-name>"))[0]["text"] == "/clear"
    assert items(entry("user", "<bash-input>ls -la</bash-input>"))[0]["text"] == "! ls -la"


def test_a_message_typed_while_claude_was_busy_is_an_attachment():
    queued = {"type": "queued_command", "commandMode": "prompt", "prompt": "also this", "origin": {"kind": "human"}}
    assert items(entry("attachment", attachment=queued)) == [
        {"id": "u1", "ts": 1791879300, "kind": "user", "text": "also this", "images": []}]
    task = {"type": "queued_command", "commandMode": "task-notification", "prompt": "<task-notification>x</task-notification>"}
    assert items(entry("attachment", attachment=task)) == []
    assert items(entry("attachment", attachment={"type": "hook_success"})) == []


def test_assistant_text_and_no_thinking():
    assert items(entry("assistant", [{"type": "text", "text": "| a |\n|---|\n"}])) == [
        {"id": "u1", "ts": 1791879300, "kind": "assistant", "text": "| a |\n|---|\n"}]
    assert items(entry("assistant", [{"type": "thinking", "thinking": "hmm"}])) == []
    assert items(entry("assistant", [{"type": "text", "text": "  "}])) == []
    assert items(entry("assistant", [{"type": "text", "text": "aside"}], isSidechain=True)) == []
    error = items(entry("assistant", [{"type": "text", "text": "API Error: 529"}], isApiErrorMessage=True))
    assert error[0]["kind"] == "note" and error[0]["text"] == "API Error: 529"


def test_blocks_of_one_entry_get_ids_of_their_own():
    got = items(entry("assistant", [{"type": "text", "text": "first"}, {"type": "text", "text": "second"},
                                    {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}]))
    assert [i["id"] for i in got] == ["u1", "u1:1", "toolu_1"]


def test_tool_call_then_its_result_as_a_patch():
    call = items(entry("assistant", [{"type": "tool_use", "id": "toolu_1", "name": "Bash",
                                      "input": {"command": "ls -la\necho done", "description": "List"}}]))
    assert call == [{"id": "toolu_1", "ts": 1791879300, "kind": "tool", "tool": "Bash", "summary": "Ran ls -la echo done",
                     "input": "ls -la\necho done", "output": None, "error": False, "images": []}]
    events, blobs = record.read(entry("user", [{"type": "tool_result", "tool_use_id": "toolu_1", "is_error": True,
                                                "content": [{"type": "text", "text": "no such file"}, image()]}], uuid="u2"))
    assert events == [{"t": "patch", "id": "toolu_1", "output": "no such file", "error": True,
                       "images": [{"id": "toolu_1-0", "mime": "image/png"}]}]
    assert blobs == [("toolu_1-0", "image/png", b"\x89PNG fake")]
    plain = items(entry("user", [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}]))
    assert plain == [{"t": "patch", "id": "toolu_1", "output": "ok", "error": False, "images": []}]


def test_summaries():
    s = record.summary
    assert s("Read", {"file_path": "/src/app.rs"}) == "Read app.rs"
    assert s("Edit", {"file_path": "/etc/config.py", "old_string": "a", "new_string": "b"}) == "Edited config.py"
    assert s("Write", {"file_path": "notes.md"}) == "Wrote notes.md"
    assert s("Grep", {"pattern": "foo"}) == 'Searched "foo"'
    assert s("WebSearch", {"query": "tmux paste"}) == 'Searched "tmux paste"'
    assert s("Agent", {"description": "Find the bug", "prompt": "long"}) == "Agent: Find the bug"
    assert s("mcp__chrome__take_screenshot", {}) == "take_screenshot"
    assert s("mcp__docs__read", {"n": 3, "ref": "abc"}) == "read abc"
    assert s("Bash", "not a dict") == "Ran"
    assert len(s("Bash", {"command": "x" * 500})) == record.SUMMARY_MAX


def test_input_and_output_are_cut():
    call = items(entry("assistant", [{"type": "tool_use", "id": "t", "name": "Write",
                                      "input": {"file_path": "a.txt", "content": "x" * 9000, "n": [1, 2]}}]))[0]
    assert call["input"].startswith("file_path: a.txt\ncontent: xxx") and len(call["input"]) == record.INPUT_MAX
    assert call["input"].endswith("…")
    assert record.readable("X", {"file_path": "a", "n": [1, 2]}) == "file_path: a\nn: [1, 2]"
    out = items(entry("user", [{"type": "tool_result", "tool_use_id": "t", "content": "y" * 9000}]))[0]
    assert len(out["output"]) == record.OUTPUT_MAX and out["output"].endswith("…")


def test_images_you_sent_and_only_real_image_types():
    events, blobs = record.read(entry("user", [image("image/jpeg"), {"type": "text", "text": "what is this"},
                                               image("image/svg+xml"), image(data="")]))
    assert events == [{"id": "u1", "ts": 1791879300, "kind": "user", "text": "what is this",
                       "images": [{"id": "u1-0", "mime": "image/jpeg"}]}]
    assert blobs == [("u1-0", "image/jpeg", b"\x89PNG fake")]
    assert items(entry("user", [image()]))[0]["text"] == ""  # an image alone is still a message


def test_notes_for_interrupts_compaction_and_what_the_reader_does_not_know():
    assert items(entry("user", [{"type": "text", "text": "[Request interrupted by user for tool use]"}])) == [
        {"id": "u1:0", "ts": 1791879300, "kind": "note", "text": "Interrupted"}]
    assert items(entry("system", subtype="compact_boundary", content="Conversation compacted"))[0] == {
        "id": "u1", "ts": 1791879300, "kind": "note", "text": "Context compacted"}
    assert items(entry("system", subtype="turn_duration")) == []
    assert items(entry("hologram"))[0] == {"id": "u1", "ts": 1791879300, "kind": "note", "text": "Unknown entry (hologram)"}
    assert items(entry("assistant", [{"type": "hologram"}]))[0]["text"] == "Unknown content (hologram)"


def test_bookkeeping_and_broken_lines_are_nothing():
    assert record.read(json.dumps({"type": "custom-title", "customTitle": "x", "sessionId": "s"})) == ([], [])
    assert record.read(json.dumps({"type": "hologram"})) == ([], [])  # no uuid: not part of the conversation
    assert record.read('{"type":"user","uuid":"u1","message":{"content":"cut o') == ([], [])
    assert record.read(b"\xff\xfe not json\n") == ([], [])
    assert record.read("[1, 2]") == ([], [])
    assert items(entry("user", "no time", timestamp=None))[0]["ts"] == 0
    assert items(json.dumps({"type": "user", "uuid": "u1", "message": "odd"})) == []


def test_fold_puts_results_into_their_calls_and_keeps_the_newest():
    events = []
    for n in range(5):
        events += items(entry("assistant", [{"type": "tool_use", "id": f"t{n}", "name": "Bash", "input": {"command": "ls"}}]))
        events += items(entry("user", [{"type": "tool_result", "tool_use_id": f"t{n}", "content": f"out{n}"}]))
    events += items(entry("user", [{"type": "tool_result", "tool_use_id": "before-the-window", "content": "lost"}]))
    events += items(entry("assistant", [{"type": "tool_use", "id": "t9", "name": "Bash", "input": {"command": "sleep 9"}}]))
    got = record.fold(events, limit=3)
    assert [(i["id"], i["output"]) for i in got] == [("t3", "out3"), ("t4", "out4"), ("t9", None)]
    assert len(record.fold(events)) == 6
