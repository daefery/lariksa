from types import SimpleNamespace

import pytest

from lariksa import overlay as overlay_mod
from lariksa.hotkey import parse
from lariksa.overlay import Overlay, pill_position


class FakeWindow:
    def __init__(self):
        self.shown = 0
        self.hidden = 0
        self.on_top = False

    def show(self):
        self.shown += 1

    def hide(self):
        self.hidden += 1

    def restore(self):
        pass


class FakeQueue:
    def __init__(self):
        self.n = 0

    def qsize(self):
        return self.n


class FakeService:
    def __init__(self):
        self.recorder = SimpleNamespace(recording=False)
        self.activity = SimpleNamespace(meeting_id="", stage="", progress=0.0)
        self._jobs = FakeQueue()
        self.settings = {"show_pill": True}
        self.store = SimpleNamespace(settings=lambda: self.settings)
        self.started = 0
        self.stopped = 0
        self.ready = True

    def status(self):
        return {"ready": self.ready}

    def start_recording(self, title):
        self.started += 1
        self.recorder.recording = True
        return {"ok": True}

    def stop_recording(self):
        self.stopped += 1
        self.recorder.recording = False
        self._jobs.n = 1  # the meeting is queued for processing


@pytest.fixture
def ov(monkeypatch):
    clock = {"t": 1000.0}
    monkeypatch.setattr(overlay_mod.time, "monotonic", lambda: clock["t"])
    o = Overlay(FakeService(), FakeWindow(), FakeWindow())
    o.clock = clock
    return o


def test_full_lifecycle_record_process_done_hide(ov):
    svc, pill = ov.service, ov.pill
    assert ov.tick(False) == "idle" and not ov._visible

    svc.start_recording("")
    assert ov.tick(False) == "recording" and ov._visible and pill.shown == 1
    assert pill.on_top is False  # never touched from the watcher thread (deadlocks WinForms)

    svc.stop_recording()
    assert ov.tick(True) == "processing" and ov._visible  # stays up while processing

    svc._jobs.n = 0
    svc.activity.meeting_id, svc.activity.stage, svc.activity.progress = "m1", "summarizing", 0.4
    assert ov.tick(False) == "processing"

    svc.activity.meeting_id = ""
    assert ov.tick(False) == "done" and ov._visible   # "Notulen siap" lingers
    ov.clock["t"] += overlay_mod.DONE_LINGER_S + 1
    assert ov.tick(False) == "idle" and not ov._visible


def test_closing_the_pill_keeps_it_closed_until_the_next_recording(ov):
    svc = ov.service
    svc.start_recording("")
    ov.tick(False)
    ov.hide_pill()
    assert not ov._visible
    svc.stop_recording()
    ov.tick(True)
    assert not ov._visible  # processing, but the user closed it
    svc._jobs.n = 0
    ov.tick(False)
    svc.start_recording("")
    ov.tick(False)
    assert ov._visible      # new recording brings it back


def test_setting_can_disable_the_pill(ov):
    ov.service.settings["show_pill"] = False
    ov.service.start_recording("")
    assert ov.tick(False) == "recording" and not ov._visible


def test_hotkey_toggles_and_opens_main_window_when_not_ready(ov):
    ov.toggle_recording()
    assert ov.service.started == 1
    ov.toggle_recording()
    assert ov.service.stopped == 1
    ov.service.ready = False
    shown = ov.main.shown
    ov.toggle_recording()
    assert ov.service.started == 1 and ov.main.shown == shown + 1


def test_pill_sits_top_centre():
    assert pill_position(0, 0, 1920) == ((1920 - 360) // 2, 16)
    assert pill_position(1920, 0, 2560) == (1920 + (2560 - 360) // 2, 16)  # second monitor


def test_hotkey_parse():
    assert parse("Ctrl+Shift+R") == (0x2 | 0x4, ord("R"))
    assert parse("alt + F10") == (0x1, 0x79)
    for bad in ("R", "Shift+R", "Ctrl+Foo", "Hyper+R"):
        with pytest.raises(ValueError):
            parse(bad)
