"""Entry point: open the Lariksa window."""

from __future__ import annotations

import logging
import sys
from logging.handlers import RotatingFileHandler

from .bridge import Bridge
from .paths import app_paths, resource
from .service import LariksaService


def _logging(paths) -> None:
    handler = RotatingFileHandler(paths.data / "lariksa.log", maxBytes=2_000_000, backupCount=2,
                                  encoding="utf-8")
    logging.basicConfig(level=logging.INFO, handlers=[handler],
                        format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    # httpx logs every request URL at INFO, including long signed download links.
    logging.getLogger("httpx").setLevel(logging.WARNING)


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
    bridge.attach(window)

    def on_closing() -> bool:
        # Never lose a meeting: closing mid-recording stops and saves it first.
        service.shutdown()
        return True

    window.events.closing += on_closing
    webview.start(private_mode=False, storage_path=str(paths.data / "webview"),
                  icon=str(resource("lariksa/ui/lariksa.ico")))
    sys.exit(0)


if __name__ == "__main__":
    main()
