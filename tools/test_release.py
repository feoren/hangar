"""Tests for the pure functions in tools/release.py.

Run: py -3 -m unittest discover tools
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

import release

THUMB = "a1b2c3d4e5f60718293a4b5c6d7e8f9011223344"


class WindowsSigningConfigTest(unittest.TestCase):
    def test_no_variables_means_unsigned(self):
        self.assertIsNone(release.windows_signing_config({}))

    def test_empty_variables_count_as_unset(self):
        env = {"HANGAR_CERT_THUMBPRINT": "  ", "HANGAR_SIGN_COMMAND": "", "HANGAR_TIMESTAMP_URL": "x"}
        self.assertIsNone(release.windows_signing_config(env))

    def test_thumbprint_with_default_timestamp(self):
        config = release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB})
        self.assertEqual(
            config,
            {
                "bundle": {
                    "windows": {
                        "certificateThumbprint": THUMB.upper(),
                        "digestAlgorithm": "sha256",
                        "timestampUrl": "http://time.certum.pl",
                        "tsp": True,
                    }
                }
            },
        )

    def test_thumbprint_from_certificate_viewer_is_cleaned(self):
        pasted = "\u200e" + " ".join(THUMB[i : i + 2] for i in range(0, 40, 2))
        config = release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": pasted})
        self.assertEqual(config["bundle"]["windows"]["certificateThumbprint"], THUMB.upper())

    def test_custom_timestamp_and_tsp_off(self):
        env = {
            "HANGAR_CERT_THUMBPRINT": THUMB,
            "HANGAR_TIMESTAMP_URL": "https://ts.example.org/rfc3161",
            "HANGAR_TIMESTAMP_TSP": "0",
        }
        windows = release.windows_signing_config(env)["bundle"]["windows"]
        self.assertEqual(windows["timestampUrl"], "https://ts.example.org/rfc3161")
        self.assertFalse(windows["tsp"])

    def test_invalid_thumbprint_without_value_in_message(self):
        secret = "zz" + THUMB[2:]
        with self.assertRaises(release.SigningError) as caught:
            release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": secret})
        self.assertNotIn(secret, str(caught.exception))
        self.assertIn("HANGAR_CERT_THUMBPRINT", str(caught.exception))

    def test_thumbprint_too_short(self):
        with self.assertRaises(release.SigningError):
            release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB[:-2]})

    def test_timestamp_must_be_http(self):
        for url in ("ftp://time.certum.pl", "time.certum.pl", "http://a b"):
            with self.subTest(url=url), self.assertRaises(release.SigningError):
                release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB, "HANGAR_TIMESTAMP_URL": url})

    def test_tsp_only_0_or_1(self):
        with self.assertRaises(release.SigningError):
            release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB, "HANGAR_TIMESTAMP_TSP": "yes"})

    def test_both_variants_exclude_each_other(self):
        env = {"HANGAR_CERT_THUMBPRINT": THUMB, "HANGAR_SIGN_COMMAND": '["signtool.exe", "sign", "%1"]'}
        with self.assertRaises(release.SigningError):
            release.windows_signing_config(env)

    def test_sign_command_as_argv(self):
        argv = ["C:\\Program Files\\Tool\\sign.exe", "sign", "/fd", "sha256", "%1"]
        config = release.windows_signing_config({"HANGAR_SIGN_COMMAND": json.dumps(argv)})
        self.assertEqual(
            config,
            {"bundle": {"windows": {"signCommand": {"cmd": argv[0], "args": argv[1:]}}}},
        )

    def test_sign_command_as_shell_string_rejected(self):
        raw = "signtool.exe sign /p secret %1"
        with self.assertRaises(release.SigningError) as caught:
            release.windows_signing_config({"HANGAR_SIGN_COMMAND": raw})
        self.assertNotIn("secret", str(caught.exception))

    def test_sign_command_malformed(self):
        cases = [
            "{}",
            '["signtool.exe"]',
            '["signtool.exe", "sign"]',
            '["signtool.exe", ""]',
            '["signtool.exe", 1, "%1"]',
            '["%1", "sign"]',
        ]
        for raw in cases:
            with self.subTest(raw=raw), self.assertRaises(release.SigningError):
                release.windows_signing_config({"HANGAR_SIGN_COMMAND": raw})

    def test_override_is_json_serializable(self):
        config = release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB})
        self.assertEqual(json.loads(json.dumps(config)), config)


class SignatureProblemTest(unittest.TestCase):
    def test_valid_with_timestamp(self):
        self.assertIsNone(release.signature_problem('{"Status":"Valid","Timestamped":true}'))

    def test_unsigned(self):
        problem = release.signature_problem('{"Status":"NotSigned","Timestamped":false}')
        self.assertIn("NotSigned", problem)

    def test_valid_without_timestamp(self):
        self.assertIn("timestamp", release.signature_problem('{"Status":"Valid","Timestamped":false}'))

    def test_malformed_output(self):
        for text in ("", "no json", "[]", '{"Status":3}'):
            with self.subTest(text=text):
                self.assertIsNotNone(release.signature_problem(text))


class BuildCommandTest(unittest.TestCase):
    """build() without a real build: the process call is intercepted."""

    def run_build(self, signing):
        calls = []

        def fake_run(command, **kwargs):
            config = None
            if "--config" in command:
                config_path = Path(command[command.index("--config") + 1])
                config = json.loads(config_path.read_text(encoding="utf-8"))
                calls.append((command, config, config_path))
            else:
                calls.append((command, None, None))
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as folder:
            key = Path(folder) / "test.key"
            key.write_text("key\n", encoding="utf-8")
            with mock.patch.object(release.shutil, "which", return_value="pnpm"), mock.patch.object(
                release.subprocess, "run", side_effect=fake_run
            ):
                release.build(key, signing)
        self.assertEqual(len(calls), 1)
        return calls[0]

    def test_unsigned_keeps_the_command(self):
        command, config, _ = self.run_build(None)
        self.assertEqual(command, ["pnpm", "tauri", "build"])
        self.assertIsNone(config)

    def test_signed_via_config_file_deleted_afterwards(self):
        signing = release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB})
        command, config, config_path = self.run_build(signing)
        self.assertEqual(command[:3], ["pnpm", "tauri", "build"])
        self.assertEqual(config, signing)
        self.assertNotIn(THUMB.upper(), " ".join(command))
        self.assertFalse(config_path.exists())


TAURI_CONF_TEXT = """{
  "productName": "Hangar",
  "version": "0.1.0",
  "bundle": {
    "targets": ["nsis"],
    "windows": { "nsis": { "languages": ["German", "English"] } }
  }
}
"""
PACKAGE_JSON_TEXT = """{
  "name": "hangar",
  "version": "0.1.0",
  "devDependencies": { "vite": "^7.0.0" }
}
"""
CARGO_TOML_TEXT = """[package]
name = "hangar"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
"""
CARGO_LOCK_TEXT = """[[package]]
name = "anyhow"
version = "1.0.0"

[[package]]
name = "hangar"
version = "0.1.0"
dependencies = [
 "anyhow",
]
"""


class ProjectFiles:
    """Creates the version files and the changelog in a folder and points release.py at them."""

    def __init__(self, folder, changelog=None):
        self.root = Path(folder)
        (self.root / "src-tauri").mkdir()
        (self.root / "src").mkdir()
        self.paths = {
            "TAURI_CONF": self.root / "src-tauri" / "tauri.conf.json",
            "PACKAGE_JSON": self.root / "package.json",
            "CARGO_TOML": self.root / "src-tauri" / "Cargo.toml",
            "CARGO_LOCK": self.root / "src-tauri" / "Cargo.lock",
            "CHANGELOG_JSON": self.root / "src" / "changelog.json",
            "RELEASE_DIR": self.root / "release",
            "ROOT": self.root,
        }
        self.paths["TAURI_CONF"].write_text(TAURI_CONF_TEXT, encoding="utf-8")
        self.paths["PACKAGE_JSON"].write_text(PACKAGE_JSON_TEXT, encoding="utf-8")
        self.paths["CARGO_TOML"].write_text(CARGO_TOML_TEXT, encoding="utf-8")
        self.paths["CARGO_LOCK"].write_text(CARGO_LOCK_TEXT, encoding="utf-8")
        release.write_json(self.paths["CHANGELOG_JSON"], changelog if changelog is not None else [])
        self.patches = [mock.patch.object(release, name, path) for name, path in self.paths.items()]

    def __enter__(self):
        for patch in self.patches:
            patch.start()
        return self

    def __exit__(self, *exc):
        for patch in reversed(self.patches):
            patch.stop()

    def text(self, name):
        return self.paths[name].read_text(encoding="utf-8")


class SetVersionTest(unittest.TestCase):
    def test_sets_all_files_and_keeps_formatting(self):
        with tempfile.TemporaryDirectory() as folder, ProjectFiles(folder) as files:
            release.set_version("0.2.0+57")
            self.assertEqual(files.text("TAURI_CONF"), TAURI_CONF_TEXT.replace("0.1.0", "0.2.0+57"))
            self.assertEqual(files.text("PACKAGE_JSON"), PACKAGE_JSON_TEXT.replace("0.1.0", "0.2.0+57"))
            self.assertEqual(files.text("CARGO_TOML"), CARGO_TOML_TEXT.replace("0.1.0", "0.2.0+57"))
            self.assertEqual(files.text("CARGO_LOCK"), CARGO_LOCK_TEXT.replace('"0.1.0"', '"0.2.0+57"'))
            self.assertEqual(release.current_version(), "0.2.0+57")

    def test_nested_version_stays_untouched(self):
        with tempfile.TemporaryDirectory() as folder, ProjectFiles(folder) as files:
            release.set_version("0.3.0")
            self.assertIn('version = "1", features', files.text("CARGO_TOML"))
            self.assertIn('name = "anyhow"\nversion = "1.0.0"', files.text("CARGO_LOCK"))


def entry(version, date=None, items=None):
    return {"version": version, "date": date, "items": [{"de": "a", "en": "a"}] if items is None else items}


class VersionRuleTest(unittest.TestCase):
    """Rule "the code always carries the upcoming version" (ADR 0002)."""

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.release_dir = Path(folder.name) / "release"
        self.entries = [entry("0.2.0"), entry("0.1.0", "2026-11-07")]

    def problem(self, version, file_version="0.2.0", entries=None):
        return release.release_problem(
            version, file_version, self.entries if entries is None else entries, self.release_dir
        )

    def test_upcoming_version_is_allowed(self):
        self.assertIsNone(self.problem("0.2.0"))
        # After a --no-build run the files already carry a build number; that does not block.
        self.assertIsNone(self.problem("0.2.0", "0.2.0+57"))

    def test_must_be_highest_changelog_version(self):
        self.assertIn("highest version", self.problem("0.3.0", "0.2.0"))
        self.assertIn("highest version", self.problem("0.1.5", "0.1.0"))
        self.assertIn("has no entry", self.problem("0.2.0", entries=[]))

    def test_not_lower_than_the_files(self):
        self.assertIn("lower than the version in the files", self.problem("0.2.0", "0.3.0"))

    def test_higher_than_published_per_changelog(self):
        entries = [entry("0.2.0", "2026-11-08"), entry("0.1.0", "2026-11-07")]
        self.assertIn("published 0.2.0", self.problem("0.2.0", "0.2.0+57", entries))

    def test_higher_than_published_per_release_folder(self):
        # The changelog date only arrives with the commit after the release; the folder exists earlier.
        (self.release_dir / "0.2.0-57").mkdir(parents=True)
        self.assertIn("published 0.2.0", self.problem("0.2.0"))

    def test_scheme(self):
        self.assertIn("X.Y.Z", self.problem("0.2"))
        self.assertIn("does not match", self.problem("0.2.0", "broken"))

    def test_last_published_from_both_sources(self):
        self.assertEqual(release.last_published(self.entries, self.release_dir), (0, 1, 0))
        (self.release_dir / "0.1.0").mkdir(parents=True)
        (self.release_dir / "0.1.4-50").mkdir()
        (self.release_dir / "notes").mkdir()
        (self.release_dir / "9.9.9-1.txt").write_text("", encoding="utf-8")
        self.assertEqual(release.last_published(self.entries, self.release_dir), (0, 1, 4))
        self.assertIsNone(release.last_published([entry("0.2.0")], Path(self.release_dir) / "missing"))


class NextVersionTest(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.release_dir = Path(folder.name) / "release"
        self.released = [entry("0.2.0", "2026-11-08"), entry("0.1.0", "2026-11-07")]

    def problem(self, version, file_version="0.2.0+57", entries=None):
        return release.next_problem(
            version, file_version, self.released if entries is None else entries, self.release_dir
        )

    def test_allowed_after_a_release(self):
        self.assertIsNone(self.problem("0.3.0"))
        self.assertIsNone(self.problem("0.2.1", "0.2.0"))

    def test_highest_version_must_be_released(self):
        self.assertIn("not released yet", self.problem("0.3.0", "0.2.0", [entry("0.2.0")]))

    def test_must_be_higher_than_everything_before(self):
        self.assertIn("higher than 0.2.0", self.problem("0.2.0"))
        self.assertIn("in the files", self.problem("0.3.0", "0.3.0"))
        (self.release_dir / "0.4.0-80").mkdir(parents=True)
        self.assertIn("published 0.4.0", self.problem("0.3.0"))
        self.assertIn("X.Y.Z", self.problem("0.3"))

    def test_suggests_next_minor_version(self):
        self.assertEqual(release.suggest_next("0.2.0"), "0.3.0")
        self.assertEqual(release.suggest_next("1.9.4"), "1.10.0")

    def test_writes_files_and_empty_entry(self):
        with tempfile.TemporaryDirectory() as folder, ProjectFiles(folder, self.released) as files:
            release.set_version("0.2.0+57")
            release.prepare_next("0.3.0")
            self.assertEqual(release.current_version(), "0.3.0")
            self.assertIn('version = "0.3.0"', files.text("CARGO_TOML"))
            changelog = json.loads(files.text("CHANGELOG_JSON"))
            self.assertEqual(changelog[0], {"version": "0.3.0", "date": None, "items": []})
            self.assertEqual(changelog[1:], self.released)

    def test_aborts_without_changing_anything(self):
        upcoming = [entry("0.2.0"), *self.released[1:]]
        with tempfile.TemporaryDirectory() as folder, ProjectFiles(folder, upcoming) as files:
            before = files.text("CHANGELOG_JSON")
            with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
                release.prepare_next("0.3.0")
            self.assertEqual(files.text("CHANGELOG_JSON"), before)
            self.assertEqual(release.current_version(), "0.1.0")


def run_shell(script, env=None):
    shell = shutil.which("pwsh") or shutil.which("powershell")
    result = subprocess.run(
        [shell, "-NoProfile", "-NonInteractive", "-Command", script],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def run_verify_script(path):
    return run_shell(release.VERIFY_SCRIPT, dict(os.environ, HANGAR_VERIFY_PATH=str(path)))


def shell_image():
    """The executable of the running shell. `shutil.which` may find a Microsoft Store execution
    alias instead (an empty reparse-point file in WindowsApps), which Get-AuthenticodeSignature
    cannot open; the process path is the signed file inside the package."""
    return Path(run_shell("(Get-Process -Id $PID).Path"))


@unittest.skipUnless(sys.platform == "win32", "Get-AuthenticodeSignature exists only on Windows")
class VerifyScriptTest(unittest.TestCase):
    """Real call to Get-AuthenticodeSignature, so the script and its evaluation stay in sync."""

    def test_unsigned_file_with_spaces_in_path(self):
        with tempfile.TemporaryDirectory() as folder:
            # A .ps1 has an Authenticode format and reports NotSigned without a signature block; a
            # made-up EXE would only yield UnknownError because it is not a valid PE file.
            path = Path(folder) / "without signature.ps1"
            path.write_text("Write-Output 1\n", encoding="utf-8")
            problem = release.signature_problem(run_verify_script(path))
        self.assertIsNotNone(problem)
        self.assertIn("NotSigned", problem)

    def test_signed_file_with_timestamp(self):
        # pwsh.exe and powershell.exe carry a timestamped Microsoft signature (embedded, or through
        # the system catalog for Windows PowerShell).
        shell = shell_image()
        self.assertTrue(shell.is_file(), shell)
        self.assertGreater(shell.stat().st_size, 0, shell)
        self.assertIsNone(release.signature_problem(run_verify_script(shell)))


class GithubReleaseTest(unittest.TestCase):
    def test_base_url_uses_the_version_tag(self):
        self.assertEqual(
            release.github_base_url("0.2.0"),
            "https://github.com/feoren/hangar/releases/download/v0.2.0",
        )

    def test_commands_publish_only_after_the_files_are_attached(self):
        target = Path("release with space") / "0.2.0-31"
        commands = release.github_release_commands(
            "0.2.0", "0.2.0+31", target, "Hangar_0.2.0-31_x64-setup.exe", target / "release-notes.md"
        )
        push, create, publish = commands
        self.assertEqual(push, ["git", "push", "origin", "main"])
        self.assertEqual(create[:4], ["gh", "release", "create", "v0.2.0"])
        self.assertIn("--draft", create)
        self.assertIn(str(target / "latest.json"), create)
        self.assertIn(str(target / "Hangar_0.2.0-31_x64-setup.exe"), create)
        self.assertEqual(create[create.index("--title") + 1], "Hangar 0.2.0-31")
        files = create[4 : create.index("--repo")]
        self.assertEqual(
            files,
            [
                str(target / "Hangar_0.2.0-31_x64-setup.exe"),
                str(target / "Hangar-Setup.exe"),
                str(target / "SHA256SUMS.txt"),
                str(target / "latest.json"),
            ],
        )
        self.assertEqual(publish[:4], ["gh", "release", "edit", "v0.2.0"])
        self.assertIn("--draft=false", publish)

    def test_quoted_keeps_paths_with_spaces_together(self):
        self.assertEqual(release.quoted(["gh", "a b", "c"]), 'gh "a b" c')


class ChecksumsTest(unittest.TestCase):
    def setUp(self):
        tmp_root = Path(__file__).resolve().parent.parent / "tests" / ".tmp"
        tmp_root.mkdir(parents=True, exist_ok=True)
        self.folder = Path(tempfile.mkdtemp(prefix="release checksums ", dir=tmp_root))
        self.addCleanup(shutil.rmtree, self.folder, ignore_errors=True)

    def test_sha256sum_format_with_lf_line_endings(self):
        (self.folder / "Hangar_0.2.0-31_x64-setup.exe").write_bytes(b"abc")
        (self.folder / "Hangar-Setup.exe").write_bytes(b"")
        release.write_checksums(self.folder, ["Hangar_0.2.0-31_x64-setup.exe", "Hangar-Setup.exe"])
        content = (self.folder / "SHA256SUMS.txt").read_bytes()
        self.assertEqual(
            content,
            b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  Hangar_0.2.0-31_x64-setup.exe\n"
            b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  Hangar-Setup.exe\n",
        )
        self.assertNotIn(b"\r", content)

    def test_release_files_end_with_checksums_then_latest_json(self):
        target = Path("release with space") / "0.2.0-31"
        names = [path.name for path in release.release_files(target, "Hangar_0.2.0-31_x64-setup.exe")]
        self.assertEqual(
            names, ["Hangar_0.2.0-31_x64-setup.exe", "Hangar-Setup.exe", "SHA256SUMS.txt", "latest.json"]
        )


class BuildNumberTest(unittest.TestCase):
    def test_base_is_added_to_the_commit_count(self):
        self.assertEqual(release.build_number_from({"buildNumberBase": 84}, 2), 86)

    def test_without_base_the_commit_count_counts(self):
        self.assertEqual(release.build_number_from({}, 7), 7)

    def test_invalid_base_aborts(self):
        for base in (-1, "84", 1.5, True):
            with self.assertRaises(SystemExit):
                release.build_number_from({"buildNumberBase": base}, 1)


if __name__ == "__main__":
    unittest.main()
