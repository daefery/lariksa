"""Render the real UI in headless Chrome with a stub API and fictional data.

    uv run python scripts/preview.py out_dir            # all scenes, 1180x780
    uv run python scripts/preview.py docs/assets --readme   # README screenshots, 2x

The page is the app's own ui/ (HTML, CSS, JS, fonts), so what you
see is what ships; only window.pywebview.api is replaced by
scripts/preview_stub.js reading scripts/sample_meeting.json.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "ui"
SCRIPTS = ROOT / "scripts"

#: name -> (query, window height)
SCENES = {
    "home": ("s=home", 780),
    "recording": ("s=recording", 780),
    "meeting": ("s=home&open=card", 780),
    "transcript": ("s=home&open=card&tab=transcript", 780),
    "processing": ("s=processing&open=processing", 780),
    "setup": ("s=setup", 780),
    "settings": ("s=home&open=settings", 780),
    "dark": ("s=home&open=card&theme=dark", 780),
}

#: README set: rendered at 2x for sharp text on high-DPI screens.
README_SCENES = {
    "screen-home": ("s=home", 780),
    "screen-recording": ("s=recording", 780),
    "screen-notulen": ("s=home&open=card", 900),
    "screen-notulen-en": ("s=home&open=card&lang=en", 900),
    "screen-transcript": ("s=home&open=card&tab=transcript", 780),
    "screen-processing": ("s=processing&open=processing", 780),
    "screen-setup": ("s=setup", 780),
    "screen-settings": ("s=home&open=settings", 900),
    "screen-dark": ("s=home&open=card&theme=dark", 780),
}


def _site(tmp: Path, meeting: Path | None = None) -> Path:
    site = tmp / "ui"
    shutil.copytree(UI, site)
    shutil.copy(SCRIPTS / "preview_stub.js", site / "stub.js")
    sample = (SCRIPTS / "sample_meeting.json").read_text(encoding="utf-8")
    inject = f"window.SAMPLE = {sample};"
    if meeting:
        inject += f"window.MEETING = {meeting.read_text(encoding='utf-8')};"
    html = (site / "index.html").read_text(encoding="utf-8")
    html = html.replace(
        '<script src="app.js">',
        f'<script>{inject}</script><script src="stub.js"></script><script src="app.js">',
    )
    # Headless Chrome freezes CSS transitions mid-way, which shows stale
    # colours after a theme switch; turn them off for screenshots only.
    html = html.replace("</head>", "<style>*,*::before,*::after{transition:none!important}</style></head>")
    (site / "index.html").write_text(html, encoding="utf-8")
    return site


def render(scenes: dict[str, tuple[str, int]], out: Path, scale: int, meeting: Path | None = None,
           width: int = 1180) -> None:
    out.mkdir(parents=True, exist_ok=True)
    chrome = shutil.which("google-chrome") or shutil.which("chromium")
    with tempfile.TemporaryDirectory() as tmp:
        site = _site(Path(tmp), meeting)
        for name, (query, height) in scenes.items():
            target = out / f"{name}.png"
            subprocess.run(
                [chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                 "--allow-file-access-from-files", "--virtual-time-budget=6000",
                 f"--force-device-scale-factor={scale}", f"--window-size={width},{height}",
                 f"--screenshot={target}", f"file://{site / 'index.html'}?{query}"],
                check=True, capture_output=True,
            )
            print(target)


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("out", type=Path, nargs="?", default=Path("preview"))
    p.add_argument("--readme", action="store_true", help="render the README screenshot set at 2x")
    p.add_argument("--meeting", type=Path, help="a meeting payload JSON to open (real data stays local)")
    p.add_argument("--height", type=int, default=1100)
    p.add_argument("--width", type=int, default=1440)
    a = p.parse_args()
    if a.meeting:
        render({"meeting-notulen": ("s=home&open=card", a.height), "meeting-transcript": ("s=home&open=card&tab=transcript", a.height)},
               a.out, scale=1, meeting=a.meeting, width=a.width)
    elif a.readme:
        render(README_SCENES, a.out, scale=2)
    else:
        render(SCENES, a.out, scale=1)


if __name__ == "__main__":
    main()
