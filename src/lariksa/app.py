"""Entry point: open the Lariksa window."""

from __future__ import annotations

import logging
import sys
from logging.handlers import RotatingFileHandler

from .bridge import Bridge
from .overlay import PILL_SIZE, Overlay, pill_position
from .paths import app_paths, resource
from .service import LariksaService


def _logging(paths) -> None:
    handler = RotatingFileHandler(paths.data / "lariksa.log", maxBytes=2_000_000, backupCount=2,
                                  encoding="utf-8")
    logging.basicConfig(level=logging.INFO, handlers=[handler],
                        format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    # httpx logs every request URL at INFO, including long signed download links.
    logging.getLogger("httpx").setLevel(logging.WARNING)


def _pill_xy(webview) -> tuple[int, int]:
    try:
        s = webview.screens()[0]
        return pill_position(s.x, s.y, s.width)
    except Exception:
        return 600, 16


def main() -> None:
    import webview

    paths = app_paths()
    _logging(paths)
    service = LariksaService(paths)
    bridge = Bridge(service)
    window = webview.create_window(
        "Lariksa",
        url=str(resource("lariksa/ui/index.html")),
        js_api=bridge,
        width=1180,
        height=780,
        min_size=(860, 600),
        background_color="#F7F8F4",
        text_select=True,
    )
    # The recording indicator: small, always on top, never takes focus from Zoom/Meet.
    x, y = _pill_xy(webview)
    pill = webview.create_window(
        "Lariksa · rekaman",
        url=str(resource("lariksa/ui/pill.html")),
        js_api=bridge,
        width=PILL_SIZE[0], height=PILL_SIZE[1], x=x, y=y,
        # resizable=True on purpose: pywebview sets FixedSingle for non-resizable
        # windows, which wraps the frameless pill in a border and crops its content.
        # The pill has no edges to grab, so it still cannot be resized by hand.
        resizable=True, frameless=True, easy_drag=False, on_top=True, focus=False,
        min_size=PILL_SIZE,
        hidden=True, background_color="#17241E", shadow=True,
    )
    overlay = Overlay(service, window, pill)
    bridge.attach(window, overlay)

    def on_started() -> None:
        overlay.attach_taskbar()
        bridge._hotkey_error = overlay.start_hotkey(service.store.settings().get("hotkey", ""))
        overlay.run()

    def on_closing() -> bool:
        # Never lose a meeting: closing mid-recording stops and saves it first.
        overlay.stop()
        service.shutdown()
        try:
            pill.destroy()
        except Exception:
            pass
        return True

    window.events.closing += on_closing
    webview.start(on_started, private_mode=False, storage_path=str(paths.data / "webview"),
                  icon=str(resource("lariksa/ui/lariksa.ico")))
    sys.exit(0)


if __name__ == "__main__":
    main()
