"""Rasterise brand/logo/*.svg: PNG sizes, a multi-size .ico, and the brand sheet.

    uv run --with pillow python brand/export_logo.py

Uses headless Chrome to render (same engine as the app's WebView2), so PNGs
match the SVGs exactly. The .ico uses the small-size master at 16/24/32 px and
the regular mark above that, which is how good app icons are hinted.
"""

from __future__ import annotations

import io
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
LOGO = ROOT / "brand" / "logo"
PNG = LOGO / "png"

ICON_SIZES = (16, 24, 32, 48, 64, 128, 180, 192, 256, 512, 1024)
SMALL_MAX = 24  # sizes up to here use mark-forest-small.svg


def _chrome() -> str:
    return shutil.which("google-chrome") or shutil.which("chromium")


def render_svg(svg: Path, width: int, height: int, target: Path) -> None:
    """Render an SVG at an exact pixel size with a transparent background."""
    with tempfile.TemporaryDirectory() as tmp:
        page = Path(tmp) / "p.html"
        page.write_text(
            f'<!doctype html><html><body style="margin:0;background:transparent">'
            f'<img src="file://{svg}" width="{width}" height="{height}" style="display:block"></body></html>',
            encoding="utf-8")
        subprocess.run([_chrome(), "--headless=new", "--disable-gpu", "--hide-scrollbars",
                        "--allow-file-access-from-files", "--default-background-color=00000000",
                        "--force-device-scale-factor=1", f"--window-size={width},{height}",
                        f"--screenshot={target}", f"file://{page}"], check=True, capture_output=True)
    # Chrome's minimum window can exceed tiny sizes; crop to the requested box.
    with Image.open(target) as im:
        im.crop((0, 0, width, height)).save(target)


def svg_size(svg: Path) -> tuple[float, float]:
    import re
    head = svg.read_text(encoding="utf-8")[:400]
    w = float(re.search(r'width="([\d.]+)"', head).group(1))
    h = float(re.search(r'height="([\d.]+)"', head).group(1))
    return w, h


def main() -> None:
    PNG.mkdir(parents=True, exist_ok=True)

    # App icon sizes (forest tile, the primary mark).
    for size in ICON_SIZES:
        src = LOGO / ("mark-forest-small.svg" if size <= SMALL_MAX else "mark-forest.svg")
        render_svg(src, size, size, PNG / f"icon-{size}.png")
    for name in ("mark-lime", "mark-paper", "mark-mono-forest", "mark-mono-white"):
        render_svg(LOGO / f"{name}.svg", 512, 512, PNG / f"{name}-512.png")

    # Lockups and wordmarks at 2x their design height, for docs and slides.
    for name in ("lockup-light", "lockup-dark", "lockup-mono-forest", "lockup-mono-white",
                 "wordmark-forest", "wordmark-white"):
        w, h = svg_size(LOGO / f"{name}.svg")
        render_svg(LOGO / f"{name}.svg", round(w * 2), round(h * 2), PNG / f"{name}@2x.png")

    # Windows .ico with hand-picked masters per size.
    frames = [Image.open(PNG / f"icon-{s}.png").convert("RGBA") for s in (16, 24, 32, 48, 64, 128, 256)]
    ico = LOGO / "lariksa.ico"
    frames[-1].save(ico, format="ICO", sizes=[f.size for f in frames], append_images=frames[:-1])
    # Pillow picks frames by resampling the largest image unless each size is given;
    # verify every size made it in.
    with Image.open(ico) as im:
        print("ico sizes:", sorted(im.info.get("sizes", [])))
    print(f"wrote {len(list(PNG.glob('*.png')))} PNGs and {ico.name}")


if __name__ == "__main__":
    main()
