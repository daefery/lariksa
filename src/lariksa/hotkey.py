"""A system-wide keyboard shortcut (Windows RegisterHotKey), no extra dependencies.

The hotkey works while Zoom or Meet has focus: RegisterHotKey delivers
WM_HOTKEY to a thread message queue owned by this process, so a small
background thread with its own message loop is enough. If the combination is
already taken by another app, registration fails and the caller is told,
rather than silently doing nothing.
"""

from __future__ import annotations

import logging
import sys
import threading
from collections.abc import Callable

log = logging.getLogger(__name__)

MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, MOD_NOREPEAT = 0x1, 0x2, 0x4, 0x8, 0x4000
WM_HOTKEY, WM_QUIT = 0x0312, 0x0012

_MODS = {"ctrl": MOD_CONTROL, "control": MOD_CONTROL, "alt": MOD_ALT, "shift": MOD_SHIFT, "win": MOD_WIN}
_KEYS = {**{chr(c): c for c in range(ord("A"), ord("Z") + 1)}, **{str(d): 0x30 + d for d in range(10)},
         **{f"F{n}": 0x6F + n for n in range(1, 13)}, "SPACE": 0x20}

#: Default: Ctrl+Shift+R ("Rekam"). Unused by Zoom, Meet in Chrome/Edge and Teams.
DEFAULT_HOTKEY = "Ctrl+Shift+R"


def parse(combo: str) -> tuple[int, int]:
    """'Ctrl+Shift+R' -> (modifiers, virtual-key). Raises ValueError on nonsense."""
    parts = [p.strip() for p in combo.replace(" ", "").split("+") if p.strip()]
    if len(parts) < 2:
        raise ValueError("Pintasan butuh minimal satu tombol pengubah dan satu tombol, misal Ctrl+Shift+R.")
    mods = 0
    for p in parts[:-1]:
        if p.lower() not in _MODS:
            raise ValueError(f"Tombol pengubah tidak dikenal: {p}")
        mods |= _MODS[p.lower()]
    key = parts[-1].upper()
    if key not in _KEYS:
        raise ValueError(f"Tombol tidak dikenal: {parts[-1]}")
    if not mods & (MOD_CONTROL | MOD_ALT | MOD_WIN):
        raise ValueError("Pakai Ctrl, Alt, atau Win agar tidak bentrok dengan mengetik.")
    return mods, _KEYS[key]


class GlobalHotkey:
    def __init__(self, combo: str, callback: Callable[[], None]):
        self.combo = combo
        self.callback = callback
        self.error = ""
        self._thread_id = 0
        self._ready = threading.Event()
        self._busy = threading.Lock()
        self._thread: threading.Thread | None = None

    def start(self) -> bool:
        """Register and listen. Returns False (with ``error`` set) if unavailable."""
        if sys.platform != "win32":
            self.error = "Pintasan global hanya tersedia di Windows."
            return False
        try:
            mods, vk = parse(self.combo)
        except ValueError as exc:
            self.error = str(exc)
            return False
        self._thread = threading.Thread(target=self._loop, args=(mods, vk), name="lariksa-hotkey", daemon=True)
        self._thread.start()
        self._ready.wait(timeout=3)
        return not self.error

    def _loop(self, mods: int, vk: int) -> None:
        import ctypes
        from ctypes import wintypes

        user32, kernel32 = ctypes.windll.user32, ctypes.windll.kernel32
        self._thread_id = kernel32.GetCurrentThreadId()
        if not user32.RegisterHotKey(None, 1, mods | MOD_NOREPEAT, vk):
            self.error = f"{self.combo} sudah dipakai aplikasi lain. Pilih pintasan lain di Pengaturan."
            self._ready.set()
            return
        log.info("Global hotkey %s registered", self.combo)
        self._ready.set()
        msg = wintypes.MSG()
        try:
            while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                if msg.message == WM_HOTKEY:
                    # Run the action on its own thread: this thread must keep pumping
                    # messages, or a slow action (opening audio devices) swallows the
                    # next press. Presses during a running action are ignored.
                    self._dispatch()
        finally:
            user32.UnregisterHotKey(None, 1)

    def _dispatch(self) -> None:
        if not self._busy.acquire(blocking=False):
            log.info("Hotkey pressed while the previous action is still running; ignored")
            return

        def run() -> None:
            try:
                self.callback()
            except Exception:  # never let a callback kill the listener
                log.exception("Hotkey callback failed")
            finally:
                self._busy.release()

        threading.Thread(target=run, name="lariksa-hotkey-action", daemon=True).start()

    def stop(self) -> None:
        if self._thread_id and sys.platform == "win32":
            import ctypes

            ctypes.windll.user32.PostThreadMessageW(self._thread_id, WM_QUIT, 0, 0)
        if self._thread:
            self._thread.join(timeout=2)
