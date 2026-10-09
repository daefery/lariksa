"""Windows taskbar progress (ITaskbarList3), so processing is visible without opening the app.

Recording shows a red "paused"-style full bar; transcription and notulen
show real progress. Pure ctypes COM, no extra dependencies; every call is a
no-op off Windows or if the shell refuses.
"""

from __future__ import annotations

import ctypes
import logging
import sys

log = logging.getLogger(__name__)

NOPROGRESS, INDETERMINATE, NORMAL, ERROR, PAUSED = 0x0, 0x1, 0x2, 0x4, 0x8

_CLSID = "{56FDF344-FD6D-11d0-958A-006097C9A090}"
_IID = "{EA1AFB91-9E28-4B86-90E9-9E9F8A5EEFAF}"
# vtable indexes: IUnknown(3) + ITaskbarList(5: HrInit, AddTab, DeleteTab, ActivateTab, SetActiveAlt)
# + ITaskbarList2(1: MarkFullscreenWindow) -> ITaskbarList3 starts at 9.
_HRINIT, _SET_VALUE, _SET_STATE = 3, 9, 10


class _GUID(ctypes.Structure):
    _fields_ = [("d1", ctypes.c_ulong), ("d2", ctypes.c_ushort), ("d3", ctypes.c_ushort), ("d4", ctypes.c_ubyte * 8)]


def _guid(text: str) -> _GUID:
    g = _GUID()
    ctypes.oledll.ole32.CLSIDFromString(ctypes.c_wchar_p(text), ctypes.byref(g))
    return g


class TaskbarProgress:
    def __init__(self) -> None:
        self._ptr = None
        self._hwnd = 0
        self._last: tuple = ()

    def attach(self, hwnd: int) -> None:
        if sys.platform != "win32" or not hwnd:
            return
        try:
            ctypes.oledll.ole32.CoInitialize(None)
        except OSError:
            pass  # already initialised on this thread
        try:
            ptr = ctypes.c_void_p()
            clsid, iid = _guid(_CLSID), _guid(_IID)
            ctypes.oledll.ole32.CoCreateInstance(ctypes.byref(clsid), None, 1, ctypes.byref(iid), ctypes.byref(ptr))
            self._ptr, self._hwnd = ptr, hwnd
            self._call(_HRINIT, ctypes.HRESULT)
        except OSError as exc:
            log.info("Taskbar progress unavailable: %s", exc)
            self._ptr = None

    def _call(self, index: int, restype, *args):
        vtable = ctypes.cast(ctypes.cast(self._ptr, ctypes.POINTER(ctypes.c_void_p))[0],
                             ctypes.POINTER(ctypes.c_void_p))
        argtypes = [ctypes.c_void_p] + [type(a) for a in args]
        fn = ctypes.WINFUNCTYPE(restype, *argtypes)(vtable[index])
        return fn(self._ptr, *args)

    def set(self, state: int, value: float = 0.0) -> None:
        """state: NOPROGRESS / INDETERMINATE / NORMAL / ERROR / PAUSED; value 0..1."""
        if not self._ptr:
            return
        key = (state, round(value, 2))
        if key == self._last:
            return
        self._last = key
        try:
            hwnd = ctypes.c_void_p(self._hwnd)
            self._call(_SET_STATE, ctypes.HRESULT, hwnd, ctypes.c_int(state))
            if state in (NORMAL, ERROR, PAUSED):
                self._call(_SET_VALUE, ctypes.HRESULT, hwnd, ctypes.c_ulonglong(int(value * 1000)),
                           ctypes.c_ulonglong(1000))
        except OSError as exc:
            log.debug("Taskbar progress call failed: %s", exc)
