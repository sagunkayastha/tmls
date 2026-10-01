import json

from tmls import host_colors


def test_host_color_is_stable_and_uses_small_named_palette():
    first = host_colors.color_for("archbox", {})
    assert first in host_colors.PALETTE.values()
    assert first == host_colors.color_for("archbox", {})
    assert host_colors.color_for("archbox", {"archbox": "green"}) == host_colors.PALETTE["green"]
    assert host_colors.color_for("ubu", {"archbox": "green"}) == host_colors.color_for("ubu", {})


def test_host_color_overrides_ignore_bad_entries(tmp_path, monkeypatch):
    monkeypatch.setattr(host_colors, "CONFIG", tmp_path / "host-colors.json")
    host_colors.CONFIG.write_text(json.dumps({"archbox": "green", "ubu": "not-a-color", "": "blue", "bad": 1}))
    assert host_colors.load() == {"archbox": "green"}
    host_colors.CONFIG.write_text("not json")
    assert host_colors.load() == {}
