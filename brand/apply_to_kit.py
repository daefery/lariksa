"""Apply the brand/ logo to Lariksa_Brand_Kit_v1.0, keeping its file names and sizes.

    uv run --with pillow --with pyoxipng python brand/apply_to_kit.py

Run after build_logo.py and export_logo.py. Every logo, icon and marketing
file in the kit is regenerated from brand/logo, so the kit and the app can
never drift apart again. The kit's tokens, copy and component notes are
left untouched.
"""

from __future__ import annotations

import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

from export_logo import render_svg, svg_size

ROOT = Path(__file__).resolve().parents[1]
LOGO = ROOT / "brand" / "logo"
PNG = LOGO / "png"
KIT = ROOT / "Lariksa_Brand_Kit_v1.0"
FONTS = ROOT / "src" / "lariksa" / "ui" / "fonts"

PAPER, MIDNIGHT = "#F7F8F4", "#17241E"


def _copy(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dst)
    print("  ", dst.relative_to(ROOT))


def _on_background(svg: Path, color: str) -> str:
    """The kit's lockup files carry their own background; keep that behaviour."""
    text = svg.read_text(encoding="utf-8")
    w, h = svg_size(svg)
    head_end = text.index("</title>") + len("</title>")
    return text[:head_end] + f'<rect width="{w}" height="{h}" fill="{color}"/>' + text[head_end:]


def logos() -> None:
    svg = KIT / "02_Logos" / "SVG"
    _copy(LOGO / "mark-forest.svg", svg / "icon_forest.svg")
    _copy(LOGO / "mark-lime.svg", svg / "icon_lime.svg")
    # The kit's "white" icon is a white tile with a forest mark.
    white = (LOGO / "mark-paper.svg").read_text(encoding="utf-8").replace(PAPER, "#FFFFFF", 1)
    (svg / "icon_white.svg").write_text(white, encoding="utf-8")
    print("  ", (svg / "icon_white.svg").relative_to(ROOT))
    _copy(LOGO / "mark-mono-forest.svg", svg / "monogram_transparent.svg")
    (svg / "lockup_light.svg").write_text(_on_background(LOGO / "lockup-light.svg", PAPER), encoding="utf-8")
    (svg / "lockup_dark.svg").write_text(_on_background(LOGO / "lockup-dark.svg", MIDNIGHT), encoding="utf-8")
    print("   lockups with background")

    png = KIT / "02_Logos" / "PNG"
    for name in ("icon_forest", "icon_lime", "icon_white"):
        render_svg(svg / f"{name}.svg", 1024, 1024, png / f"{name}.png")
    for name in ("lockup_light", "lockup_dark"):
        w, h = svg_size(svg / f"{name}.svg")
        render_svg(svg / f"{name}.svg", round(w * 2), round(h * 2), png / f"{name}.png")
    print("   02_Logos/PNG rendered")


def app_icons() -> None:
    icons = KIT / "03_App_Icons"
    for size in (16, 32, 64, 128, 180, 192, 512, 1024):
        _copy(PNG / f"icon-{size}.png", icons / f"lariksa_icon_{size}px.png")
    _copy(LOGO / "mark-forest-small.svg", icons / "favicon.svg")
    _copy(LOGO / "lariksa.ico", icons / "favicon.ico")


def social() -> None:
    """Rebuild the 1200x630 card with the new mark and the real wordmark."""
    lock = (LOGO / "lockup-dark.svg").read_text(encoding="utf-8")
    mark = (LOGO / "mark-lime.svg").read_text(encoding="utf-8")
    # Inline SVGs keep their width/height attributes over CSS boxes; size them explicitly.
    mark_inline = mark.replace('width="512" height="512"', 'width="176" height="176"', 1)
    html = f"""<!doctype html><html><head><meta charset="utf-8"><style>
@font-face{{font-family:J;src:url({FONTS / 'PlusJakartaSans.woff2'}) format("woff2");font-weight:200 800}}
*{{margin:0;box-sizing:border-box}}
body{{width:1200px;height:630px;background:#17241E;color:#fff;font-family:J;position:relative;overflow:hidden}}
.rings{{position:absolute;inset:0}}
.lock{{position:absolute;left:66px;top:62px;height:78px}}
.lock svg{{height:78px;width:auto}}
h1{{position:absolute;left:92px;top:224px;font-size:76px;line-height:1.02;letter-spacing:-3px;font-weight:800;max-width:720px}}
h1 em{{font-style:normal;color:#C9F27A}}
p{{position:absolute;left:94px;top:470px;font-size:26px;color:#BACABF;max-width:720px;line-height:1.4}}
.mark{{position:absolute;right:112px;bottom:72px;width:176px;height:176px}}
</style></head><body>
<svg class="rings" viewBox="0 0 1200 630"><circle cx="1060" cy="96" r="260" fill="none" stroke="#2B3D33" stroke-width="2"/><circle cx="1030" cy="96" r="190" fill="none" stroke="#2B3D33" stroke-width="2"/></svg>
<div class="lock">{lock}</div>
<h1>Meeting notes that <em>never leave</em> your laptop.</h1>
<p>Record any call. Get the notulen when you hang up.</p>
<div class="mark">{mark_inline}</div>
</body></html>"""
    out = KIT / "06_Marketing_Templates"
    with tempfile.TemporaryDirectory() as tmp:
        page = Path(tmp) / "social.html"
        page.write_text(html, encoding="utf-8")
        chrome = shutil.which("google-chrome") or shutil.which("chromium")
        subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                        "--allow-file-access-from-files", "--virtual-time-budget=3000",
                        "--force-device-scale-factor=1", "--window-size=1200,630",
                        f"--screenshot={out / 'social_1200x630.png'}", f"file://{page}"],
                       check=True, capture_output=True)
    # An SVG twin that needs no fonts: the same layout with the outlined lockup.
    lock_body = lock[lock.index("</title>") + 8:lock.rindex("</svg>")]
    lw, lh = svg_size(LOGO / "lockup-dark.svg")
    mark_body = mark[mark.index("</title>") + 8:mark.rindex("</svg>")]
    scale = 78 / lh
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">'
        '<title>Lariksa social card</title><rect width="1200" height="630" fill="#17241E"/>'
        '<circle cx="1060" cy="96" r="260" fill="none" stroke="#2B3D33" stroke-width="2"/>'
        '<circle cx="1030" cy="96" r="190" fill="none" stroke="#2B3D33" stroke-width="2"/>'
        f'<g transform="translate(66 62) scale({scale:.4f})">{lock_body}</g>'
        '<text x="92" y="300" fill="#FFFFFF" font-family="Plus Jakarta Sans, Arial, sans-serif" font-size="76" '
        'font-weight="800" letter-spacing="-3">Meeting notes that</text>'
        '<text x="92" y="378" font-family="Plus Jakarta Sans, Arial, sans-serif" font-size="76" font-weight="800" '
        'letter-spacing="-3"><tspan fill="#C9F27A">never leave</tspan><tspan fill="#FFFFFF"> your</tspan></text>'
        '<text x="92" y="456" fill="#FFFFFF" font-family="Plus Jakarta Sans, Arial, sans-serif" font-size="76" '
        'font-weight="800" letter-spacing="-3">laptop.</text>'
        '<text x="94" y="510" fill="#BACABF" font-family="Plus Jakarta Sans, Arial, sans-serif" font-size="26">'
        'Record any call. Get the notulen when you hang up.</text>'
        f'<g transform="translate(912 382) scale({176 / 64:.4f})">{mark_body}</g></svg>\n'
    )
    (out / "social_1200x630.svg").write_text(svg, encoding="utf-8")
    print("   06_Marketing_Templates/social_1200x630.{png,svg}")


def preview_page() -> None:
    """Swap the kit's UI concept wordmark for the real mark + name."""
    page = KIT / "05_UI_Components" / "workspace-preview.html"
    s = page.read_text(encoding="utf-8")
    old = '<div class="brand">lariksa.</div>'
    if old in s:
        mark = (LOGO / "mark-forest.svg").read_text(encoding="utf-8")
        inline = mark.replace('width="512" height="512"', 'width="30" height="30" style="vertical-align:-6px;margin-right:8px"')
        s = s.replace(old, f'<div class="brand">{inline.strip()}lariksa.</div>')
        page.write_text(s, encoding="utf-8")
        print("  ", page.relative_to(ROOT))


def readme_note() -> None:
    readme = KIT / "README.md"
    s = readme.read_text(encoding="utf-8")
    note = ("\n## Logo update\n"
            "The logo files in 02, 03 and 06 now carry the **\"l.\" mark** from `brand/` in the repository "
            "(a soft l that becomes a written line and ends in a lime full stop). They are generated by "
            "`brand/apply_to_kit.py`; edit `brand/build_logo.py`, not these files.\n")
    if "## Logo update" not in s:
        readme.write_text(s.rstrip() + "\n" + note, encoding="utf-8")
        print("  ", readme.relative_to(ROOT))


def optimise() -> None:
    try:
        import oxipng
    except ImportError:
        return
    for png in KIT.rglob("*.png"):
        oxipng.optimize(png, level=4, strip=oxipng.StripChunks.safe())


def main() -> None:
    print("logos");      logos()
    print("app icons");  app_icons()
    print("social");     social()
    print("preview");    preview_page()
    readme_note()
    optimise()
    # Sanity: every PNG kept the size the kit promised.
    for png in sorted(KIT.rglob("*.png")):
        with Image.open(png) as im:
            print(f"   {png.relative_to(KIT)} {im.size}")


if __name__ == "__main__":
    main()
