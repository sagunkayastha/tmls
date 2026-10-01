import json

from tmls import notifications


def test_notification_switches_persist(tmp_path, monkeypatch):
    monkeypatch.setattr(notifications, "CONFIG", tmp_path / "notifications.json")
    settings = notifications.load()
    assert settings.desktop and not settings.sound and settings.silence_focused
    settings.sound = True
    settings.desktop = False
    settings.silence_focused = False
    notifications.save(settings)
    assert notifications.load() == settings
    assert json.loads(notifications.CONFIG.read_text()) == {
        "desktop": False, "sound": True, "silence_focused": False}


async def test_notification_commands_are_separate_and_only_for_actionable_marks(monkeypatch):
    commands = []

    async def run(*argv):
        commands.append(argv)
    monkeypatch.setattr(notifications, "_run", run)
    settings = notifications.Settings(desktop=True, sound=True, silence_focused=True)
    await notifications.emit(settings, "archbox", "Season-36", "waiting", "permission prompt")
    assert commands == [
        ("notify-send", "tmls · archbox", "? Season-36 · permission prompt"),
        ("canberra-gtk-play", "-i", "message-new-instant", "-d", "tmls"),
    ]
    commands.clear()
    await notifications.emit(settings, "archbox", "Season-36", "failed", None)
    assert commands == []
