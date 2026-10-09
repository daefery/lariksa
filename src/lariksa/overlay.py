"""The always-on-top recording pill, the global shortcut and taskbar progress.

One small watcher thread follows the service state and keeps the three in
sync, so it does not matter how a recording started or stopped (main window
button, shortcut, pill button):

    recording            -> pill shows timer + levels + Selesai; taskbar red bar
    processing           -> pill shows stage + %; taskbar green progress
    done                 -> pill says "Notulen siap" for a few seconds, then hides
    pill closed by user  -> stays hidden until the next recording

The pill never steals focus from the meeting app (``focus=False``).
"""

from __future__ import annotations

import logging
import threading
import time

from . import taskbar
from .hotkey import GlobalHotkey

log = logging.getLogger(__name__)

PILL_SIZE = (360, 58)
MARGIN = 16
DONE_LINGER_S = 6.0


def pill_position(screen_x: int, screen_y: int, screen_w: int, pill_w: int = PILL_SIZE[0]) -> tuple[int, int]:
    """Top centre of the primary screen, just below the edge (meeting apps put controls at the bottom)."""
    return screen_x + (screen_w - pill_w) // 2, screen_y + MARGIN


class Overlay:
    def __init__(self, service, main_window, pill_window):
        self.service = service
        self.main = main_window
        self.pill = pill_window
        self.hotkey: GlobalHotkey | None = None
        self.taskbar = taskbar.TaskbarProgress()
        self._dismissed = False   # user closed the pill after a recording
        self._visible = False
        self._done_at = 0.0
        self._active = False      # recording or processing seen since the last "done"
        self._stop = threading.Event()

    # -- shortcut -------------------------------------------------------

    def start_hotkey(self, combo: str) -> str:
        """(Re)register the global shortcut. Returns "" or an error message for the UI."""
        if self.hotkey:
            self.hotkey.stop()
            self.hotkey = None
        if not combo:
            return ""
        hk = GlobalHotkey(combo, self.toggle_recording)
        if hk.start():
            self.hotkey = hk
            return ""
        log.warning("Hotkey %s unavailable: %s", combo, hk.error)
        return hk.error

    @property
    def hotkey_label(self) -> str:
        return self.hotkey.combo if self.hotkey else ""

    def toggle_recording(self) -> None:
        log.info("Shortcut pressed (recording=%s)", self.service.recorder.recording)
        if self.service.recorder.recording:
            self.service.stop_recording()
            return
        if not self.service.status().get("ready"):
            self.show_main()  # models missing: the main window explains what to do
            return
        result = self.service.start_recording("")
        if not result.get("ok"):
            log.warning("Hotkey start failed: %s", result.get("error"))
            self.show_main()

    # -- windows ----------------------------------------------------------

    def show_main(self) -> None:
        try:
            self.main.restore()
            self.main.show()
        except Exception:  # pragma: no cover - window already gone
            pass

    def hide_pill(self) -> None:
        self._dismissed = True
        self._set_pill(False)

    def _set_pill(self, visible: bool) -> None:
        if visible == self._visible:
            return
        self._visible = visible
        # Only show()/hide() here: they marshal to the GUI thread. Setting WinForms
        # properties such as TopMost from this thread deadlocks the UI (seen on
        # Windows); the window is created on_top=True and stays that way.
        try:
            if visible:
                self._show_no_activate()
            else:
                self.pill.hide()
        except Exception:  # pragma: no cover - during shutdown
            pass

    def _show_no_activate(self) -> None:
        """Show the pill without taking focus from the meeting app.

        pywebview's show() calls Form.Activate(), which would pull keyboard focus
        away from Zoom/Meet mid-sentence. ShowWindow(SW_SHOWNOACTIVATE) is a plain
        Win32 call that is safe from any thread and keeps the topmost style.
        """
        import sys

        hwnd = self._pill_hwnd()
        if sys.platform == "win32" and hwnd:
            import ctypes

            SW_SHOWNOACTIVATE, HWND_TOPMOST = 4, -1
            SWP_NOMOVE, SWP_NOACTIVATE = 0x2, 0x10
            user32 = ctypes.windll.user32
            user32.ShowWindow(hwnd, SW_SHOWNOACTIVATE)
            # Size in physical pixels for this monitor's DPI, so the pill is never cropped.
            scale = (user32.GetDpiForWindow(hwnd) or 96) / 96
            user32.SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, int(PILL_SIZE[0] * scale), int(PILL_SIZE[1] * scale),
                                SWP_NOMOVE | SWP_NOACTIVATE)
        else:
            self.pill.show()

    def _pill_hwnd(self) -> int:
        try:
            return int(self.pill.native.Handle.ToInt64())
        except Exception:
            return 0

    # -- watcher ------------------------------------------------------------

    def attach_taskbar(self) -> None:
        try:
            hwnd = int(self.main.native.Handle.ToInt64())
        except Exception:
            return
        self.taskbar.attach(hwnd)

    def run(self) -> None:
        threading.Thread(target=self._watch, name="lariksa-overlay", daemon=True).start()

    def stop(self) -> None:
        self._stop.set()
        if self.hotkey:
            self.hotkey.stop()

    def _watch(self) -> None:
        was_recording = False
        while not self._stop.wait(0.5):
            try:
                self.tick(was_recording)
                was_recording = self.service.recorder.recording
            except Exception:  # never let the indicator take the app down
                log.exception("Overlay tick failed")

    def tick(self, was_recording: bool) -> str:
        """Advance the indicator state; returns the state name (for tests)."""
        svc = self.service
        recording = svc.recorder.recording
        busy = bool(svc.activity.meeting_id) or svc._jobs.qsize() > 0
        settings = svc.store.settings()
        wanted = settings.get("show_pill", True)

        if recording and not was_recording:
            self._dismissed = False  # a new recording always brings the pill back
        if recording:
            state = "recording"
            self._active = True
            self.taskbar.set(taskbar.ERROR, 1.0)
        elif busy:
            state = "processing"
            self._active = True
            progress = svc.activity.progress if svc.activity.stage != "queued" else 0.0
            self.taskbar.set(taskbar.NORMAL if progress else taskbar.INDETERMINATE, progress)
        elif self._active:
            # Work just finished: say so briefly, then get out of the way.
            self._active = False
            self._done_at = time.monotonic()
            state = "done"
            self.taskbar.set(taskbar.NOPROGRESS)
        elif self._done_at and time.monotonic() - self._done_at < DONE_LINGER_S:
            state = "done"
        else:
            state = "idle"
            self._done_at = 0.0
            self.taskbar.set(taskbar.NOPROGRESS)

        self._set_pill(wanted and not self._dismissed and state != "idle")
        return state
