"""Shrinks sub-agent figure images for the app bundle.

Reads every PNG, WebP or GIF in the input folder and writes a square WebP of SIZE pixels with
alpha into the output folder, named after the source (`backend-01.png` -> `backend-01.webp`).
Only names of the form `<role>-<nn>` are taken, so the app can match them to agent names.

    py -3 tools/figures.py <input folder> <output folder> [--size 256] [--quality 82]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from PIL import Image

SIZE = 256
QUALITY = 82
NAME = re.compile(r"^[a-z]+(?:-[a-z]+)*-\d{2}$")
SOURCES = {".png", ".webp", ".gif"}


def figure_names(folder: Path) -> list[Path]:
    """Source images whose stem is a figure name, sorted by name."""
    return sorted(
        path
        for path in folder.iterdir()
        if path.is_file() and path.suffix.lower() in SOURCES and NAME.match(path.stem)
    )


def shrink(source: Path, target: Path, size: int = SIZE, quality: int = QUALITY) -> int:
    """Writes `source` as a `size` x `size` WebP with alpha, centered; returns its byte count."""
    with Image.open(source) as image:
        image = image.convert("RGBA")
        image.thumbnail((size, size), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        canvas.paste(image, ((size - image.width) // 2, (size - image.height) // 2))
        target.parent.mkdir(parents=True, exist_ok=True)
        canvas.save(target, "WEBP", quality=quality, method=6, exact=False)
    return target.stat().st_size


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--size", type=int, default=SIZE)
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
        written = shrink(source, target, args.size, args.quality)
        print(f"{target.name}: {written // 1024} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
