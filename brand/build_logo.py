"""Build the Lariksa logo system as plain SVG (no fonts, no strokes, no masks).

    uv run --with fonttools --with brotli python brand/build_logo.py

The mark: a soft "l" that turns into a written line (larik = a line of
writing) and ends in a lime full stop, the moment the minutes are done.
The wordmark: "lariksa" outlined from Plus Jakarta Sans ExtraBold (SIL OFL
1.1, which permits use in logos), tightened, with the same lime full stop.

Everything is generated from the constants below, so every file in
brand/logo/ stays consistent. Shapes are filled outlines: they render the
same in browsers, Figma, Illustrator, Inkscape and Office.
"""

from __future__ import annotations

import math
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "brand" / "logo"
FONT = ROOT / "src" / "lariksa" / "ui" / "fonts" / "PlusJakartaSans.woff2"

# Brand Kit v1.0 colours.
FOREST, MIDNIGHT, LIME, PAPER, WHITE = "#23362C", "#17241E", "#C9F27A", "#F7F8F4", "#FFFFFF"

# ---------------------------------------------------------------------------
# The mark, on a 64-unit grid.
# ---------------------------------------------------------------------------
GRID = 64
TILE_RADIUS = 15          # rounded square, ~23% like the app icon
STROKE = 7.5              # line weight
X, TOP, BASE = 19.0, 15.0, 45.0
TURN = 10.0               # radius of the turn into the baseline
LINE_END = 35.0           # where the written line stops
DOT = (44.5, 45.0, 4.6)   # the full stop: cx, cy, r

TRADEMARK_TITLE = "Lariksa"


def _arc(cx: float, cy: float, r: float, start: float, end: float, steps: int) -> list[tuple[float, float]]:
    return [(cx + r * math.cos(a), cy + r * math.sin(a))
            for a in (start + (end - start) * i / steps for i in range(steps + 1))]


def mark_path(stroke: float = STROKE, line_end: float = LINE_END) -> str:
    """The stroked 'l' as one filled outline: round caps, a soft turn, no SVG strokes.

    Built as the centre line offset by half the stroke on both sides, with
    exact circular arcs, so it scales and exports like any other shape.
    """
    h = stroke / 2
    outer_r, inner_r = TURN + h, TURN - h
    cx, cy = X + TURN, BASE - TURN  # centre of the turn
    # Outer contour: down the left edge, around the turn's outside, along the bottom.
    d = [f"M{X - h:.3f} {TOP:.3f}"]
    d.append(f"V{cy:.3f}")
    d.append(f"A{outer_r:.3f} {outer_r:.3f} 0 0 0 {cx:.3f} {BASE + h:.3f}")
    d.append(f"H{line_end:.3f}")
    d.append(f"A{h:.3f} {h:.3f} 0 0 0 {line_end:.3f} {BASE - h:.3f}")  # round end cap
    d.append(f"H{cx:.3f}")
    d.append(f"A{inner_r:.3f} {inner_r:.3f} 0 0 1 {X + h:.3f} {cy:.3f}")
    d.append(f"V{TOP:.3f}")
    d.append(f"A{h:.3f} {h:.3f} 0 0 0 {X - h:.3f} {TOP:.3f}Z")  # round top cap
    return " ".join(d)


#: Small sizes (16-24 px) get optical compensation: a heavier line and a
#: bigger full stop, so the stop still reads as a dot and not a speck.
SMALL_STROKE, SMALL_DOT_R, SMALL_LINE_END, SMALL_SHIFT = 10.0, 7.0, 31.0, -3.0


def mark_svg(tile: str | None, line: str, dot: str, *, size: int = 512, small: bool = False,
             title: str = TRADEMARK_TITLE) -> str:
    cx, cy, r = DOT
    stroke, line_end = STROKE, LINE_END
    if small:
        stroke, r, line_end = SMALL_STROKE, SMALL_DOT_R, SMALL_LINE_END
        # Gap from cap end to dot equals the stroke width, so it stays open at 16 px.
        cx = line_end + stroke / 2 + stroke * 0.8 + r
        cy = BASE + stroke / 2 - r
    shift = SMALL_SHIFT if small else 0.0
    bg = f'<rect width="{GRID}" height="{GRID}" rx="{TILE_RADIUS}" fill="{tile}"/>' if tile else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {GRID} {GRID}" '
            f'role="img" aria-label="{title}"><title>{title}</title>{bg}<g transform="translate({shift} 0)">'
            f'<path d="{mark_path(stroke, line_end)}" fill="{line}"/><circle cx="{cx}" cy="{cy}" r="{r}" fill="{dot}"/>'
            f'</g></svg>\n')


# ---------------------------------------------------------------------------
# The wordmark, outlined from the font.
# ---------------------------------------------------------------------------
WEIGHT = 800
TRACKING = -0.045         # em, the brand's tight display setting
WORD = "lariksa"


def _font() -> TTFont:
    font = TTFont(FONT)
    return instantiateVariableFont(font, {"wght": WEIGHT}, inplace=False)


def wordmark_paths() -> tuple[list[str], float, float, float]:
    """Glyph outlines for 'lariksa', in font units with y pointing down.

    Returns (paths, advance_width, x_height, baseline_y) where the outline sits
    on y=0 baseline after flipping, and the advance includes tracking.
    """
    font = _font()
    upm = font["head"].unitsPerEm
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    track = TRACKING * upm
    paths, x = [], 0.0
    for i, ch in enumerate(WORD):
        name = cmap[ord(ch)]
        pen = SVGPathPen(glyphs)
        # Flip y (font units point up) and place at x.
        glyphs[name].draw(TransformPen(pen, (1, 0, 0, -1, x, 0)))
        paths.append(pen.getCommands())
        x += hmtx[name][0] + (track if i < len(WORD) - 1 else 0)
    # Trim the last glyph's right side bearing so the full stop spacing is optical.
    last = cmap[ord(WORD[-1])]
    bp = BoundsPen(glyphs)
    glyphs[last].draw(bp)
    x_end = x - hmtx[last][0] + bp.bounds[2]
    return paths, x_end, font["OS/2"].sxHeight, 0.0


def lockup_svg(*, mark_tile: str | None, mark_line: str, mark_dot: str, text: str, dot: str, bg: str | None,
               height: int = 240) -> str:
    """Horizontal lockup: [mark] lariksa.  Everything aligned to the x-height."""
    paths, word_w, x_height, _ = wordmark_paths()
    # Scale the type so its x-height equals 50% of the tile height (optical match).
    tile = 152.0
    scale = (tile * 0.50) / x_height
    pad = (height - tile) / 2
    gap = tile * 0.24
    word_x = pad + tile + gap
    baseline = pad + tile / 2 + (x_height * scale) / 2  # centre the x-height on the tile
    dot_r = DOT[2] / GRID * tile * 0.98
    dot_x = word_x + word_w * scale + dot_r * 1.55
    width = dot_x + dot_r + pad
    cx, cy, r = DOT
    k = tile / GRID
    mark_bg = f'<rect x="{pad}" y="{pad}" width="{tile}" height="{tile}" rx="{TILE_RADIUS * k:.2f}" fill="{mark_tile}"/>' if mark_tile else ""
    back = f'<rect width="{width:.1f}" height="{height}" fill="{bg}"/>' if bg else ""
    word = "".join(f'<path d="{p}"/>' for p in paths)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{height}" viewBox="0 0 {width:.1f} {height}" '
        f'role="img" aria-label="{TRADEMARK_TITLE}"><title>{TRADEMARK_TITLE}</title>{back}{mark_bg}'
        f'<g transform="translate({pad} {pad}) scale({k:.4f})"><path d="{mark_path()}" fill="{mark_line}"/>'
        f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{mark_dot}"/></g>'
        f'<g transform="translate({word_x:.2f} {baseline:.2f}) scale({scale:.5f})" fill="{text}">{word}</g>'
        f'<circle cx="{dot_x:.2f}" cy="{baseline - dot_r:.2f}" r="{dot_r:.2f}" fill="{dot}"/></svg>\n'
    )


def wordmark_svg(*, text: str, dot: str, bg: str | None, height: int = 160) -> str:
    """The word alone: 'lariksa.' with the lime stop."""
    paths, word_w, x_height, _ = wordmark_paths()
    font = _font()
    asc = font["OS/2"].sTypoAscender
    scale = (height * 0.62) / asc
    pad_x = height * 0.18
    baseline = height * 0.5 + (x_height * scale) / 2 + height * 0.06
    dot_r = x_height * scale * 0.20
    dot_x = pad_x + word_w * scale + dot_r * 1.55
    width = dot_x + dot_r + pad_x
    back = f'<rect width="{width:.1f}" height="{height}" fill="{bg}"/>' if bg else ""
    word = "".join(f'<path d="{p}"/>' for p in paths)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{height}" viewBox="0 0 {width:.1f} {height}" '
        f'role="img" aria-label="{TRADEMARK_TITLE}"><title>{TRADEMARK_TITLE}</title>{back}'
        f'<g transform="translate({pad_x:.2f} {baseline:.2f}) scale({scale:.5f})" fill="{text}">{word}</g>'
        f'<circle cx="{dot_x:.2f}" cy="{baseline - dot_r:.2f}" r="{dot_r:.2f}" fill="{dot}"/></svg>\n'
    )


# ---------------------------------------------------------------------------
# Files
# ---------------------------------------------------------------------------
FILES = {
    # Marks
    "mark-forest.svg": lambda: mark_svg(FOREST, PAPER, LIME),       # primary app icon
    "mark-lime.svg": lambda: mark_svg(LIME, MIDNIGHT, MIDNIGHT),    # on dark backgrounds
    "mark-paper.svg": lambda: mark_svg(PAPER, FOREST, FOREST),      # on photos / busy UI
    "mark-forest-small.svg": lambda: mark_svg(FOREST, PAPER, LIME, size=32, small=True),  # favicon, 16-24 px
    "mark-mono-forest.svg": lambda: mark_svg(None, FOREST, FOREST), # no tile, one colour
    "mark-mono-white.svg": lambda: mark_svg(None, WHITE, WHITE),
    # Horizontal lockups. Lime appears once: in the mark on light, in the stop on dark.
    "lockup-light.svg": lambda: lockup_svg(mark_tile=FOREST, mark_line=PAPER, mark_dot=LIME,
                                           text=FOREST, dot=FOREST, bg=None),
    "lockup-dark.svg": lambda: lockup_svg(mark_tile=LIME, mark_line=MIDNIGHT, mark_dot=MIDNIGHT,
                                          text=WHITE, dot=LIME, bg=None),
    "lockup-mono-forest.svg": lambda: lockup_svg(mark_tile=FOREST, mark_line=WHITE, mark_dot=WHITE,
                                                 text=FOREST, dot=FOREST, bg=None),
    "lockup-mono-white.svg": lambda: lockup_svg(mark_tile=WHITE, mark_line=MIDNIGHT, mark_dot=MIDNIGHT,
                                                text=WHITE, dot=WHITE, bg=None),
    # Wordmark only
    "wordmark-forest.svg": lambda: wordmark_svg(text=FOREST, dot=FOREST, bg=None),
    "wordmark-white.svg": lambda: wordmark_svg(text=WHITE, dot=LIME, bg=None),
}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, build in FILES.items():
        (OUT / name).write_text(build(), encoding="utf-8")
        print(OUT / name)


if __name__ == "__main__":
    main()
