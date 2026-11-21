"""Tests for figures.py; files only under tests/.tmp/ in the repository."""

from __future__ import annotations

import re
import shutil
import sys
import unittest
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))

import figures  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
TMP = ROOT / "tests" / ".tmp" / "figures"
BUNDLED = ROOT / "src" / "assets" / "sub-agents"
LIBRARY = ROOT / "src" / "subAgentLibrary.ts"


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
        self.picture("n8n-01.png", (64, 64))
        self.picture("8ball-01.png", (64, 64))
        self.picture("Backend.png", (64, 64))
        self.picture("notes.png", (64, 64))
        (self.source / "tester-01.txt").write_text("x")
        names = [path.name for path in figures.figure_names(self.source)]
        self.assertEqual(names, ["backend-01.png", "code-reviewer-02.png", "n8n-01.png"])

    def test_output_is_a_portrait_webp_with_alpha(self) -> None:
        self.picture("tester-01.png", (600, 300))
        self.assertEqual(figures.main([str(self.source), str(self.target), "--height", "120"]), 0)
        out = self.target / "tester-01.webp"
        with Image.open(out) as image:
            self.assertEqual(image.format, "WEBP")
            self.assertEqual(image.size, (80, 120))
            self.assertEqual(image.convert("RGBA").getpixel((0, 0))[3], 0)

    def test_one_source_per_name_png_first(self) -> None:
        self.picture("sales-01.webp", (64, 64))
        self.picture("sales-01.png", (64, 64))
        self.picture("tester-02.gif", (64, 64))
        self.picture("tester-02.webp", (64, 64))
        names = [path.name for path in figures.figure_names(self.source)]
        self.assertEqual(names, ["sales-01.png", "tester-02.webp"])

    def test_same_master_in_two_formats_gives_the_same_file(self) -> None:
        self.picture("sales-01.png", (300, 600))
        figures.main([str(self.source), str(self.target), "--height", "120"])
        once = (self.target / "sales-01.webp").read_bytes()
        with Image.open(self.source / "sales-01.png") as image:
            image.save(self.source / "sales-01.webp", "WEBP", quality=50)
        figures.main([str(self.source), str(self.target), "--height", "120"])
        self.assertEqual((self.target / "sales-01.webp").read_bytes(), once)

    def test_a_wide_figure_sits_lower(self) -> None:
        self.picture("tall-01.png", (200, 800))
        self.picture("wide-01.png", (800, 200))
        figures.main([str(self.source), str(self.target)])
        self.assertEqual(figures.top_of(self.target / "tall-01.webp"), figures.PADDING)
        self.assertEqual(figures.figure_tops(self.target).keys(), {"wide-01"})

    def test_bundled_tops_match_the_app_list(self) -> None:
        source = LIBRARY.read_text(encoding="utf-8")
        block = re.search(r"export const FIGURE_TOPS[^{]*\{(.*?)\};", source, re.S)
        self.assertIsNotNone(block)
        assert block is not None
        entries = re.findall(r'"?([a-z0-9-]+)"?: (\d+),', block.group(1))
        listed = {name: int(top) for name, top in entries}
        self.assertEqual(listed, figures.figure_tops(BUNDLED))

    def test_missing_or_empty_input_fails(self) -> None:
        self.assertEqual(figures.main([str(TMP / "nowhere"), str(self.target)]), 2)
        self.assertEqual(figures.main([str(self.source), str(self.target)]), 1)


if __name__ == "__main__":
    unittest.main()
