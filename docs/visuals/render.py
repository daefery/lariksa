"""Render the README visuals into docs/assets/.

    uv run python docs/visuals/render.py

Step 1 renders the real app UI with fictional data (scripts/preview.py
--readme). Step 2 renders the composed brand pages in this folder, which embed
those screenshots, at 2x for sharp text on high-DPI screens.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
ASSETS = ROOT / "docs" / "assets"

#: page -> (width, height) in CSS pixels; must match each page's body size.
PAGES = {
    "hero": (1600, 820),
    "flow": (1600, 760),
    "features": (1600, 900),
    "screens": (1600, 1020),
    "languages": (1600, 780),
    "architecture": (1600, 840),
}


def main() -> None:
    subprocess.run([sys.executable, str(ROOT / "scripts" / "preview.py"), str(ASSETS), "--readme"], check=True)
    chrome = shutil.which("google-chrome") or shutil.which("chromium")
    for name, (width, height) in PAGES.items():
        target = ASSETS / f"{name}.png"
        subprocess.run(
            [chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
             "--virtual-time-budget=4000", "--force-device-scale-factor=2", f"--window-size={width},{height}",
             f"--screenshot={target}", f"file://{HERE / (name + '.html')}"],
            check=True, capture_output=True,
        )
        print(target)
    _optimise(ASSETS)


def _optimise(folder: Path) -> None:
    """Lossless PNG recompression (oxipng) when available; README pages load faster."""
    try:
        import oxipng
    except ImportError:
        print("oxipng not installed; skipping compression (uv run --with pyoxipng ...)")
        return
    for png in sorted(folder.glob("*.png")):
        before = png.stat().st_size
        oxipng.optimize(png, level=4, strip=oxipng.StripChunks.safe())
        print(f"{png.name}: {before // 1024} KB -> {png.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
