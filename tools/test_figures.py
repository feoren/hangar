"""Tests for figures.py; files only under tests/.tmp/ in the repository."""

from __future__ import annotations

import shutil
import sys
import unittest
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))

import figures  # noqa: E402

TMP = Path(__file__).resolve().parent.parent / "tests" / ".tmp" / "figures"


class FiguresTest(unittest.TestCase):
    def setUp(self) -> None:
        shutil.rmtree(TMP, ignore_errors=True)
        self.source = TMP / "in"
        self.target = TMP / "out"
        self.source.mkdir(parents=True)

    def tearDown(self) -> None:
        shutil.rmtree(TMP, ignore_errors=True)

    def picture(self, name: str, size: tuple[int, int]) -> Path:
        path = self.source / name
        image = Image.new("RGBA", size, (0, 0, 0, 0))
        image.paste((200, 40, 40, 255), (size[0] // 4, size[1] // 4, size[0] // 2, size[1] // 2))
        image.save(path)
        return path

    def test_only_figure_names_are_taken(self) -> None:
        self.picture("backend-01.png", (64, 64))
        self.picture("code-reviewer-02.png", (64, 64))
        self.picture("Backend.png", (64, 64))
        self.picture("notes.png", (64, 64))
        (self.source / "tester-01.txt").write_text("x")
        names = [path.name for path in figures.figure_names(self.source)]
        self.assertEqual(names, ["backend-01.png", "code-reviewer-02.png"])

    def test_output_is_a_square_webp_with_alpha(self) -> None:
        self.picture("tester-01.png", (600, 300))
        self.assertEqual(figures.main([str(self.source), str(self.target), "--size", "128"]), 0)
        out = self.target / "tester-01.webp"
        with Image.open(out) as image:
            self.assertEqual(image.format, "WEBP")
            self.assertEqual(image.size, (128, 128))
            self.assertEqual(image.convert("RGBA").getpixel((0, 0))[3], 0)

    def test_missing_or_empty_input_fails(self) -> None:
        self.assertEqual(figures.main([str(TMP / "nowhere"), str(self.target)]), 2)
        self.assertEqual(figures.main([str(self.source), str(self.target)]), 1)


if __name__ == "__main__":
    unittest.main()
