"""Guard: Python tests keep their files under tests/.tmp/ in the repository, never in the system
temp folder. tools/test_release.py has the helper (`test_folder`)."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent

# tempfile calls that default to the system temp folder; mkdtemp only without `dir=`.
FORBIDDEN = re.compile(
    r"\b(TemporaryDirectory|NamedTemporaryFile|SpooledTemporaryFile|TemporaryFile|gettempdir"
    r"|mkstemp)\(|\bmkdtemp\((?![^)]*\bdir=)"
)


def reaches_system_temp(line: str) -> bool:
    """A comment line may name the call (to warn against it); only code counts."""
    return not line.lstrip().startswith("#") and FORBIDDEN.search(line) is not None


class TempFilesTest(unittest.TestCase):
    def test_tests_do_not_use_the_system_temp_folder(self) -> None:
        files = [path for path in TOOLS.rglob("test_*.py") if path != Path(__file__).resolve()]
        self.assertTrue(files)
        hits = []
        for path in files:
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                if reaches_system_temp(line):
                    hits.append(f"{path.name}:{number}: {line.strip()}")
        self.assertEqual(hits, [], "test files belong under tests/.tmp (test_folder)")

    def test_pattern_catches_the_system_temp_calls(self) -> None:
        for line in (
            "with tempfile.TemporaryDirectory() as folder:",
            "folder = tempfile.mkdtemp(prefix='x')",
            "handle = tempfile.NamedTemporaryFile('w')",
            "root = tempfile.gettempdir()",
        ):
            with self.subTest(line=line):
                self.assertTrue(reaches_system_temp(line))
        self.assertFalse(reaches_system_temp("folder = tempfile.mkdtemp(prefix=prefix, dir=tmp_root)"))
        self.assertFalse(reaches_system_temp("    # never tempfile.TemporaryDirectory() here"))


if __name__ == "__main__":
    unittest.main()
