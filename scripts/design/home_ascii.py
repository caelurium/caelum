#!/usr/bin/env python3
"""Bake Home's per-character ink colours and tiny monospace glyph atlas (Pillow)."""

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "apps/desktop/assets"
GLYPHS = " .:-=+*#%@"
CELL = (16, 28)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font", type=Path, required=True, help="Monospace TrueType font")
    args = parser.parse_args()
    source = ASSETS / "home/black-hole.txt"
    reference = ASSETS / "black-hole-ascii.png"
    rows = source.read_text().splitlines()
    width, height = 300, 100
    assert max(map(len, rows)) == width and len(rows) <= height
    rows += [""] * (height - len(rows))
    assert set("".join(rows)) <= set(GLYPHS)
    image = Image.open(reference).convert("RGB")
    colours = bytearray()
    for y, row in enumerate(rows):
        for x, char in enumerate(row.ljust(width)):
            if char == " ":
                colours.extend((0, 0, 0))
                continue
            box = (
                round(x * image.width / width),
                round(y * image.height / height),
                round((x + 1) * image.width / width),
                round((y + 1) * image.height / height),
            )
            pixels = sorted(image.crop(box).get_flattened_data(), key=lambda p: sum(p), reverse=True)
            ink = pixels[:max(1, len(pixels) // 12)]
            colours.extend(round(sum(p[c] for p in ink) / len(ink)) for c in range(3))
    (ASSETS / "home/black-hole.rgb").write_bytes(colours)

    font = ImageFont.truetype(str(args.font), 20)
    atlas = Image.new("L", (CELL[0] * len(GLYPHS), CELL[1]))
    draw = ImageDraw.Draw(atlas)
    for i, char in enumerate(GLYPHS):
        draw.text((i * CELL[0] + CELL[0] / 2, 21), char, font=font, fill=255, anchor="ms")
    (ASSETS / "home/glyphs.r8").write_bytes(atlas.tobytes())
    metadata = {
        "grid": [width, height],
        "reference_size": list(image.size),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "reference_sha256": hashlib.sha256(reference.read_bytes()).hexdigest(),
        "glyphs": GLYPHS,
        "glyph_cell": list(CELL),
        "font": list(font.getname()),
        "font_size": 20,
    }
    (ASSETS / "home/metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Baked {width}×{height} colour cells and {len(GLYPHS)} glyphs")


if __name__ == "__main__":
    main()
