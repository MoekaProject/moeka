#!/usr/bin/env python3
"""Slice the 3x3 app-icon grid (assets/icons/app/icons.png) into 9 tiles.

Each tile is a full-bleed macOS-style app icon: the card artwork is cropped to
a centered square that sits fully inside the card, then clipped to a rounded
square using the standard macOS corner radius. The result is written as
`assets/icons/app/icon-{index}.png`, index 0..8 in row-major order (top-left is
0, bottom-right is 8). The rounded square fills the whole tile canvas (no
padding) so the settings picker can render it edge-to-edge; the extra margin a
Dock icon needs is added later when the .icns is generated.

Run from the repository root:

    python3 scripts/slice_app_icons.py
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

GRID = 3
REPO_ROOT = Path(__file__).resolve().parent.parent
SOURCE = REPO_ROOT / "assets" / "icons" / "app" / "icons.png"
OUT_DIR = SOURCE.parent

# How close (Euclidean RGB distance) a pixel must be to the sampled corner mat
# color to count as card artwork ("non-background").
BG_TOLERANCE = 44.0
# Standard macOS icon corner radius as a fraction of the icon side.
CORNER_RATIO = 0.2237
# Trim a few pixels off every side so the square stays fully inside the card and
# the straight edges never pick up the surrounding mat.
EDGE_SAFETY = 3
# Supersampling factor used when rasterizing the rounded-rectangle mask.
SUPERSAMPLE = 4


def sample_background_color(rgb: np.ndarray) -> np.ndarray:
    """Average the mat color from the four 8x8 corner patches of ``rgb``."""
    patches = np.concatenate(
        [
            rgb[0:8, 0:8].reshape(-1, 3),
            rgb[0:8, -8:].reshape(-1, 3),
            rgb[-8:, 0:8].reshape(-1, 3),
            rgb[-8:, -8:].reshape(-1, 3),
        ],
        axis=0,
    )
    return patches.mean(axis=0)


def detect_bands(mask_1d: np.ndarray, threshold: float = 0.15) -> list[tuple[int, int]]:
    """Return contiguous (start, end) bands where ``mask_1d`` exceeds threshold."""
    on = mask_1d > threshold
    bands: list[tuple[int, int]] = []
    start = None
    for i, value in enumerate(on):
        if value and start is None:
            start = i
        elif not value and start is not None:
            bands.append((start, i - 1))
            start = None
    if start is not None:
        bands.append((start, len(on) - 1))
    return bands


def build_rounded_mask(side: int, radius: int) -> Image.Image:
    """Rasterize an anti-aliased full-bleed rounded-square alpha mask."""
    scale = SUPERSAMPLE
    big = Image.new("L", (side * scale, side * scale), 0)
    draw = ImageDraw.Draw(big)
    draw.rounded_rectangle(
        [0, 0, side * scale - 1, side * scale - 1],
        radius=radius * scale,
        fill=255,
    )
    return big.resize((side, side), Image.LANCZOS)


def build_tile(image: Image.Image, center: tuple[int, int], side: int, mask: Image.Image) -> Image.Image:
    """Crop a centered square for one card and clip it to the rounded-square mask."""
    center_x, center_y = center
    box_left = center_x - side // 2
    box_top = center_y - side // 2
    crop = image.crop((box_left, box_top, box_left + side, box_top + side))
    red, green, blue, _ = crop.split()
    return Image.merge("RGBA", (red, green, blue, mask))


def main() -> int:
    if not SOURCE.exists():
        print(f"error: source grid not found: {SOURCE}", file=sys.stderr)
        return 1

    image = Image.open(SOURCE).convert("RGBA")
    rgb = np.asarray(image, dtype=np.float32)[:, :, :3]

    # Locate the real card positions from the artwork projections instead of an
    # even split; the cards do not perfectly align to thirds and are separated
    # by thin mat gaps.
    background_color = sample_background_color(rgb)
    non_bg = np.sqrt(((rgb - background_color) ** 2).sum(axis=2)) > BG_TOLERANCE
    col_bands = detect_bands(non_bg.mean(axis=0))
    row_bands = detect_bands(non_bg.mean(axis=1))
    if len(col_bands) != GRID or len(row_bands) != GRID:
        print(
            f"error: expected a {GRID}x{GRID} grid but found "
            f"{len(col_bands)} columns and {len(row_bands)} rows",
            file=sys.stderr,
        )
        return 1

    # Largest square that stays fully inside every card, so each card fills its
    # tile with no surrounding mat on the straight edges.
    min_span = min(
        min(right - left for left, right in col_bands),
        min(bottom - top for top, bottom in row_bands),
    )
    side = min_span - 2 * EDGE_SAFETY
    radius = round(side * CORNER_RATIO)
    mask = build_rounded_mask(side, radius)

    for row, (top, bottom) in enumerate(row_bands):
        for col, (left, right) in enumerate(col_bands):
            index = row * GRID + col
            center = ((left + right) // 2, (top + bottom) // 2)
            tile = build_tile(image, center, side, mask)
            out_path = OUT_DIR / f"icon-{index}.png"
            tile.save(out_path)
            print(f"wrote {out_path.relative_to(REPO_ROOT)} ({tile.width}x{tile.height})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
