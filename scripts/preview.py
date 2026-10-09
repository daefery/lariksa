"""Render UI screenshots in headless Chrome using the stub API.

    uv run python scripts/preview.py out_dir
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "src" / "lariksa" / "ui"

SHOTS = {
    "home": "s=home",
    "recording": "s=recording",
    "meeting": "s=home&open=card",
    "transcript": "s=home&open=card&tab=transcript",
    "processing": "s=processing&open=processing",
    "setup": "s=setup",
    "settings": "s=home&open=settings",
    "dark": "s=home&open=card&theme=dark",
}


def main() -> None:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "preview")
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        site = Path(tmp) / "ui"
        shutil.copytree(UI, site)
        shutil.copy(ROOT / "scripts" / "preview_stub.js", site / "stub.js")
        html = (site / "index.html").read_text(encoding="utf-8")
        (site / "index.html").write_text(
            # Headless Chrome freezes CSS transitions mid-way, which shows stale
            # colours after a theme switch; turn them off for screenshots only.
            html.replace('<script src="app.js">', '<script src="stub.js"></script><script src="app.js">')
                .replace("</head>", "<style>*,*::before,*::after{transition:none!important}</style></head>"),
            encoding="utf-8")
        chrome = shutil.which("google-chrome") or shutil.which("chromium")
        for name, query in SHOTS.items():
            url = f"file://{site / 'index.html'}?{query}"
            subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                            "--allow-file-access-from-files", "--virtual-time-budget=6000",
                            "--window-size=1180,780", f"--screenshot={out / (name + '.png')}", url],
                           check=True, capture_output=True)
            print(out / f"{name}.png")


if __name__ == "__main__":
    main()
