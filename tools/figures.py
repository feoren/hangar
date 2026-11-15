"""Shrinks sub-agent figure images for the app bundle.

Reads every PNG, WebP or GIF in the input folder and writes a portrait WebP with alpha into the
output folder, named after the source (`backend-01.png` -> `backend-01.webp`). The empty margin
around each figure is cut off and the figure fills the canvas height, standing centered on its
bottom edge (the app puts it on a plinth), so the head sits at the same height in every figure
(the head crop of small places relies on it). Only names of the form `<role>-<nn>` are taken, so
the app can match them to agent names.

    py -3 tools/figures.py <input folder> <output folder> [--height 360] [--quality 82]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from PIL import Image

HEIGHT = 360
# Width : height of the canvas, 2 : 3. The app draws the figure in a box of the same shape.
ASPECT = (2, 3)
QUALITY = 82
# Alpha above this counts as part of the figure when the empty margin is measured.
VISIBLE = 8
PADDING = 2
NAME = re.compile(r"^[a-z]+(?:-[a-z]+)*-\d{2}$")
SOURCES = {".png", ".webp", ".gif"}


def figure_names(folder: Path) -> list[Path]:
    """Source images whose stem is a figure name, sorted by name."""
    return sorted(
        path
        for path in folder.iterdir()
        if path.is_file() and path.suffix.lower() in SOURCES and NAME.match(path.stem)
    )


def width_for(height: int) -> int:
    return height * ASPECT[0] // ASPECT[1]


def bounds(image: Image.Image) -> tuple[int, int, int, int]:
    """Box around the visible pixels; the whole image when it has none."""
    alpha = image.convert("RGBA").getchannel("A").point(lambda v: 255 if v > VISIBLE else 0)
    return alpha.getbbox() or (0, 0, image.width, image.height)


def shrink(source: Path, target: Path, height: int = HEIGHT, quality: int = QUALITY) -> int:
    """Writes `source` cropped to its figure, as large as the portrait canvas allows and standing
    on its bottom edge, as a WebP with alpha; returns its byte count."""
    width = width_for(height)
    with Image.open(source) as image:
        image = image.convert("RGBA")
        image = image.crop(bounds(image))
        scale = min((height - 2 * PADDING) / image.height, (width - 2 * PADDING) / image.width)
        size = (max(1, round(image.width * scale)), max(1, round(image.height * scale)))
        image = image.resize(size, Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
        canvas.paste(image, ((width - image.width) // 2, height - PADDING - image.height))
        target.parent.mkdir(parents=True, exist_ok=True)
        canvas.save(target, "WEBP", quality=quality, method=6, exact=False)
    return target.stat().st_size


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--height", type=int, default=HEIGHT)
    parser.add_argument("--quality", type=int, default=QUALITY)
    args = parser.parse_args(argv)
    if not args.input.is_dir():
        print(f"not a folder: {args.input}", file=sys.stderr)
        return 2
    sources = figure_names(args.input)
    if not sources:
        print(f"no figure images (<role>-<nn>.png) in {args.input}", file=sys.stderr)
        return 1
    for source in sources:
        target = args.output / f"{source.stem}.webp"
        written = shrink(source, target, args.height, args.quality)
        print(f"{target.name}: {written // 1024} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
