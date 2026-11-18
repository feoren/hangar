"""Tests for the pure functions in tools/release.py.

Run: py -3 -m unittest discover tools
"""
import base64
import contextlib
import io
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


def test_folder(test, prefix):
    tmp_root = Path(__file__).resolve().parent.parent / "tests" / ".tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix=prefix, dir=tmp_root))
    test.addCleanup(shutil.rmtree, folder, ignore_errors=True)
    return folder


# What a release machine may have set; none of it may reach the build.
SIGNING_ENVIRONMENT = {
    "TAURI_SIGNING_PRIVATE_KEY": "inherited-key-marker",
    "TAURI_SIGNING_PRIVATE_KEY_PATH": "inherited-path-marker",
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "inherited-password-marker",
}


class BuildCommandTest(unittest.TestCase):
    """build() without a real build: the process call is intercepted."""

    def run_build(self, signing, environ=None):
        calls = []

        def fake_run(command, **kwargs):
            config = None
            config_path = None
            if "--config" in command:
                config_path = Path(command[command.index("--config") + 1])
                config = json.loads(config_path.read_text(encoding="utf-8"))
            calls.append((command, config, config_path, kwargs.get("env")))
            return subprocess.CompletedProcess(command, 0)

        environ = dict(os.environ, **(environ or {}))
        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release, "cli_problem", return_value=None
        ), mock.patch.object(release.subprocess, "run", side_effect=fake_run), mock.patch.dict(
            release.os.environ, environ, clear=True
        ):
            release.build(signing)
        self.assertEqual(len(calls), 1)
        return calls[0]

    def test_unsigned_only_switches_off_updater_artifacts(self):
        command, config, config_path, _ = self.run_build(None)
        self.assertEqual(command[:3], [NODE, str(release.TAURI_JS), "build"])
        self.assertEqual(config, {"bundle": {"createUpdaterArtifacts": False}})
        self.assertFalse(config_path.exists())

    def test_signed_via_config_file_deleted_afterwards(self):
        signing = release.windows_signing_config({"HANGAR_CERT_THUMBPRINT": THUMB})
        command, config, config_path, _ = self.run_build(signing)
        self.assertEqual(command[:3], [NODE, str(release.TAURI_JS), "build"])
        self.assertEqual(config["bundle"]["windows"], signing["bundle"]["windows"])
        self.assertIs(config["bundle"]["createUpdaterArtifacts"], False)
        self.assertNotIn(THUMB.upper(), " ".join(command))
        self.assertFalse(config_path.exists())

    def test_the_build_never_sees_the_updater_key(self):
        _, _, _, env = self.run_build(None, SIGNING_ENVIRONMENT)
        self.assertIsNotNone(env)
        self.assertEqual([name for name in env if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], [])
        self.assertNotIn("inherited-key-marker", env.values())
        # The rest of the environment still reaches the build.
        self.assertEqual(env.get("PATH"), os.environ.get("PATH"))


NODE = r"C:\Program Files\nodejs\node.exe"
CARGO = r"C:\Users\builder\.cargo\bin\cargo.exe"


def fake_tool(*names):
    """Stands in for resolve_tool: fixed absolute programs, no PATH lookup."""
    return {"node": NODE, "cargo": CARGO, "git": r"C:\Program Files\Git\cmd\git.exe"}[names[0]]


def minisign_text(comment, key_id, payload_length):
    """A throwaway minisign public key or signature in Tauri's base64 form: only the shape and the
    key id are real, the key material is random bytes."""
    blob = b"Ed" + key_id + os.urandom(payload_length)
    text = "untrusted comment: %s\n%s\n" % (comment, base64.b64encode(blob).decode("ascii"))
    return base64.b64encode(text.encode("utf-8")).decode("ascii")


def throwaway_key_pair(folder, key_id):
    """Public key text and the file a signature made with it would have, under tests/.tmp."""
    public = minisign_text("minisign public key", key_id, 32)
    (folder / "throwaway.pub").write_text(public, encoding="utf-8")
    signature = minisign_text("signature from tauri secret key", key_id, 64)
    return public, signature


class TauriCommandTest(unittest.TestCase):
    def test_the_cli_runs_through_node_never_through_a_cmd_shim(self):
        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release, "cli_problem", return_value=None
        ):
            command = release.tauri_command("build")
        self.assertEqual(command, [NODE, str(release.TAURI_JS), "build"])

    def test_a_cli_that_does_not_match_the_lock_file_is_refused(self):
        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release, "cli_problem", return_value="mismatch"
        ), mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            release.tauri_command("build")


LOCK_TEXT = """lockfileVersion: '9.0'

importers:

  .:
    devDependencies:
      '@tauri-apps/api':
        specifier: ^2.10.0
        version: 2.10.1
      '@tauri-apps/cli':
        specifier: ^2.12.4
        version: 2.12.4

packages:

  '@tauri-apps/cli@2.12.4':
    resolution: {integrity: sha512-marker}
"""


class CliVersionTest(unittest.TestCase):
    def test_the_locked_version_is_read_from_the_root_importer(self):
        self.assertEqual(release.locked_cli_version(LOCK_TEXT), "2.12.4")
        self.assertIsNone(release.locked_cli_version("lockfileVersion: '9.0'\n"))

    def test_installed_cli_must_match_the_lock_file(self):
        self.assertIsNone(release.cli_version_problem('{"version": "2.12.4"}', LOCK_TEXT))
        self.assertIn("2.12.5", release.cli_version_problem('{"version": "2.12.5"}', LOCK_TEXT))
        self.assertIsNotNone(release.cli_version_problem("not json", LOCK_TEXT))
        self.assertIsNotNone(release.cli_version_problem('{"version": "2.12.4"}', ""))

    def test_the_checked_out_cli_matches(self):
        if not (release.ROOT / "node_modules" / "@tauri-apps" / "cli" / "package.json").is_file():
            self.skipTest("node_modules not installed")
        self.assertIsNone(release.cli_problem())


class FindProgramTest(unittest.TestCase):
    """Programs come from absolute PATH folders outside the repository and the working folder."""

    def setUp(self):
        self.folder = test_folder(self, "release path ")
        self.tools = self.folder / "tools"
        self.planted = self.folder / "planted"
        self.tools.mkdir()
        self.planted.mkdir()

    def find(self, name, entries, excluded=(), windows=True):
        search_path = os.pathsep.join(str(entry) for entry in entries)
        return release.find_program(name, search_path, list(excluded), windows)

    def test_only_an_exe_counts_on_windows(self):
        (self.tools / "node.cmd").write_bytes(b"")
        (self.tools / "node.com").write_bytes(b"")
        self.assertIsNone(self.find("node", [self.tools]))
        (self.tools / "node.exe").write_bytes(b"")
        self.assertEqual(self.find("node", [self.tools]), str((self.tools / "node.exe").resolve()))

    def test_excluded_folders_and_relative_entries_are_skipped(self):
        for folder in (self.tools, self.planted):
            (folder / "git.exe").write_bytes(b"")
        found = self.find("git", ["", ".", "planted", self.planted, self.tools], excluded=[self.planted])
        self.assertEqual(found, str((self.tools / "git.exe").resolve()))
        self.assertIsNone(self.find("git", [self.planted], excluded=[self.folder]))

    def test_a_working_folder_above_the_path_folder_does_not_hide_it(self):
        # Started from the home folder, ~/.cargo/bin or AppData\...\nodejs lie below the working
        # folder; Windows only searches the working folder itself, not its subfolders.
        program = "cargo.exe" if sys.platform == "win32" else "cargo"
        (self.tools / program).write_bytes(b"")
        (self.tools / program).chmod(0o755)
        (self.folder / program).write_bytes(b"")
        previous = os.getcwd()
        os.chdir(self.folder)
        self.addCleanup(os.chdir, previous)
        with mock.patch.object(release, "ROOT", self.planted), mock.patch.dict(
            release.os.environ, {"PATH": os.pathsep.join([str(self.folder), str(self.tools)])}
        ):
            self.assertEqual(release.resolve_tool("cargo"), str((self.tools / program).resolve()))

    @unittest.skipIf(sys.platform == "win32", "execute permission exists only on POSIX")
    def test_posix_needs_an_executable_file(self):
        program = self.tools / "git"
        program.write_bytes(b"")
        program.chmod(0o644)
        self.assertIsNone(self.find("git", [self.tools], windows=False))
        program.chmod(0o755)
        self.assertEqual(self.find("git", [self.tools], windows=False), str(program.resolve()))


def on_path(name):
    program = name + ".exe" if sys.platform == "win32" else name
    entries = os.environ.get("PATH", "").split(os.pathsep)
    return any(os.path.isabs(entry) and Path(entry, program).is_file() for entry in entries)


class CurrentFolderProgramTest(unittest.TestCase):
    """A file-write-only agent in the repository plants node.com or git.exe in the working
    folder; Windows searched it before PATH (shutil.which and CreateProcess both did)."""

    def setUp(self):
        self.folder = test_folder(self, "release cwd ")
        for name in ("node.com", "node.exe", "git.exe", "git.com"):
            (self.folder / name).write_bytes(b"")
        previous = os.getcwd()
        os.chdir(self.folder)
        self.addCleanup(os.chdir, previous)

    def assert_from_path(self, program):
        self.assertTrue(os.path.isabs(program), program)
        self.assertFalse(release.is_inside(Path(program), self.folder), program)
        self.assertFalse(release.is_inside(Path(program), release.ROOT), program)
        if sys.platform == "win32":
            self.assertTrue(program.endswith(".exe"), program)

    def test_node_for_the_tauri_cli_comes_from_path(self):
        if not on_path("node"):
            self.skipTest("node not installed")
        with mock.patch.object(release, "cli_problem", return_value=None):
            command = release.tauri_command("build")
        self.assert_from_path(command[0])

    def test_git_comes_from_path(self):
        if not on_path("git"):
            self.skipTest("git not installed")
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

        with mock.patch.object(release.subprocess, "run", side_effect=fake_run):
            release.git("status", "--porcelain")
        self.assert_from_path(calls[0][0])


class SignatureKeyTest(unittest.TestCase):
    def setUp(self):
        self.folder = test_folder(self, "release key ")

    def test_the_key_id_is_read_from_both_forms(self):
        public, signature = throwaway_key_pair(self.folder, bytes.fromhex("0102030405060708"))
        # minisign shows the id as a little-endian number, so the bytes appear reversed.
        self.assertEqual(release.minisign_key_id(public), "0807060504030201")
        self.assertEqual(release.minisign_key_id(signature), "0807060504030201")
        self.assertIsNone(release.signature_key_problem(signature, public))

    def test_the_shipped_key_is_readable(self):
        # The same id the untrusted comment of the shipped public key names.
        self.assertEqual(release.minisign_key_id(release.updater_pubkey()), "E34C168FC5B115C8")
        public = base64.b64decode(release.updater_pubkey()).decode("utf-8")
        self.assertTrue(public.splitlines()[0].endswith("E34C168FC5B115C8"))

    def test_another_key_is_refused(self):
        public, _ = throwaway_key_pair(self.folder, bytes.fromhex("0102030405060708"))
        _, other = throwaway_key_pair(self.folder, bytes.fromhex("1112131415161718"))
        problem = release.signature_key_problem(other, public)
        self.assertIn("1817161514131211", problem)

    def test_garbage_is_refused(self):
        public, _ = throwaway_key_pair(self.folder, bytes(8))
        for text in ("signature-marker", "", base64.b64encode(b"one line").decode()):
            self.assertIsNotNone(release.signature_key_problem(text, public))
        self.assertIsNone(release.minisign_key_id("not base64 !"))


class UpdaterSigningTest(unittest.TestCase):
    """sign_update(): a process of its own, the only one that gets the key."""

    def setUp(self):
        self.folder = test_folder(self, "release signing ")
        self.key = self.folder / "test key.key"
        self.key.write_text("key-content-marker\n", encoding="utf-8")
        self.setup = self.folder / "Hangar_0.2.0+31_x64-setup.exe"
        self.setup.write_bytes(b"installer")
        self.public, self.signature = throwaway_key_pair(self.folder, bytes.fromhex("A1A2A3A4A5A6A7A8"))

    def run_sign(
        self,
        environ=None,
        writes_signature=True,
        returncode=0,
        signature=None,
        password="pw-marker",
        allow_unencrypted_key=False,
        expected_sha256=None,
        changes_installer=False,
    ):
        calls = []
        self.calls = calls
        self.prompts = []
        expected = expected_sha256 or release.sha256_hex(self.setup)

        def fake_run(command, **kwargs):
            calls.append((command, kwargs.get("env")))
            if writes_signature:
                Path(command[-1] + ".sig").write_text(signature or self.signature, encoding="utf-8")
            if changes_installer:
                Path(command[-1]).write_bytes(b"installer changed by the signer")
            return subprocess.CompletedProcess(command, returncode)

        clean = {
            name: value
            for name, value in os.environ.items()
            if not name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")
        }
        clean.update(environ or {})

        def fake_prompt(prompt=""):
            self.prompts.append(prompt)
            return password

        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release, "cli_problem", return_value=None
        ), mock.patch.object(release.subprocess, "run", side_effect=fake_run), mock.patch.dict(
            release.os.environ, clean, clear=True
        ), mock.patch.object(release, "updater_pubkey", return_value=self.public), mock.patch.object(
            release.getpass, "getpass", side_effect=fake_prompt
        ):
            signed = release.sign_update(
                self.key, self.setup, "0.2.0+31", expected, allow_unencrypted_key=allow_unencrypted_key
            )
            self.environ_after = dict(release.os.environ)
        return calls[0], signed

    def printed(self, stream):
        return "".join(str(call.args[0]) for call in stream.write.call_args_list if call.args)

    def test_signs_the_installer_bound_to_the_version(self):
        (command, env), signed = self.run_sign()
        self.assertEqual(
            command,
            [NODE, str(release.TAURI_JS), "signer", "sign", "--app-version", "0.2.0+31", str(self.setup)],
        )
        self.assertEqual(signed.signature, self.setup.with_name(self.setup.name + ".sig"))
        self.assertEqual(signed.text, self.signature)
        self.assertEqual(signed.sha256, release.sha256_hex(self.setup))
        self.assertEqual(env["TAURI_SIGNING_PRIVATE_KEY"], "key-content-marker")
        self.assertEqual(env["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"], "pw-marker")
        # Neither key nor password on the command line, where the process list shows them.
        self.assertNotIn("key-content-marker", " ".join(command))
        self.assertNotIn("pw-marker", " ".join(command))

    def test_the_password_is_asked_for_once(self):
        self.run_sign()
        self.assertEqual(len(self.prompts), 1)

    def test_the_password_reaches_only_the_signer(self):
        self.run_sign()
        self.assertEqual(len(self.calls), 1)
        self.assertNotIn("pw-marker", self.environ_after.values())
        self.assertEqual(
            [name for name in self.environ_after if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], []
        )

    def test_an_empty_password_aborts_before_the_signer_runs(self):
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            self.run_sign(password="")
        self.assertEqual(self.calls, [])
        self.assertIn("--allow-unencrypted-key", self.printed(err))

    def test_an_unencrypted_key_signs_only_with_the_flag(self):
        with mock.patch("sys.stdout") as out:
            (_, env), _ = self.run_sign(password="", allow_unencrypted_key=True)
        self.assertEqual(self.prompts, [])
        self.assertEqual(env["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"], "")
        self.assertIn("no password", self.printed(out))

    def test_a_password_in_the_environment_is_refused(self):
        # Even with the flag: whatever ran in that shell before could read it.
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            self.run_sign({"tauri_signing_private_key_password": "env-pw-marker"}, allow_unencrypted_key=True)
        self.assertEqual(self.calls, [])
        self.assertIn("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", self.printed(err))
        self.assertNotIn("env-pw-marker", self.printed(err))
        self.assertIsNotNone(release.unlock_variable_problem({"TAURI_SIGNING_PRIVATE_KEY_PASSWORD": ""}))
        self.assertIsNone(release.unlock_variable_problem({"TAURI_SIGNING_PRIVATE_KEY_PATH": "x"}))

    def test_an_inherited_key_path_does_not_win_over_the_chosen_key(self):
        (_, env), _ = self.run_sign({"TAURI_SIGNING_PRIVATE_KEY_PATH": "inherited-path-marker"})
        self.assertNotIn("TAURI_SIGNING_PRIVATE_KEY_PATH", env)

    def test_a_stale_signature_never_counts(self):
        stale = self.setup.with_name(self.setup.name + ".sig")
        stale.write_text("old", encoding="utf-8")
        with self.assertRaises(SystemExit):
            self.run_sign(writes_signature=False)
        self.assertFalse(stale.exists())

    def test_a_failed_signer_aborts(self):
        with self.assertRaises(SystemExit):
            self.run_sign(returncode=1)

    def test_node_settings_never_reach_the_signer(self):
        injected = {
            "NODE_OPTIONS": "--require=planted-marker",
            "NODE_PATH": "planted-marker",
            "npm_config_prefix": "planted-marker",
            "HANGAR_CERT_THUMBPRINT": THUMB,
        }
        (_, env), _ = self.run_sign(injected)
        self.assertNotIn("planted-marker", " ".join(env.values()))
        self.assertNotIn(THUMB, env.values())
        self.assertEqual(env["NoDefaultCurrentDirectoryInExePath"], "1")
        if "PATH" in os.environ:
            self.assertEqual(env["PATH"], os.environ["PATH"])

    def test_an_installer_changed_after_its_authenticode_check_is_never_signed(self):
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            self.run_sign(expected_sha256="0" * 64)
        self.assertEqual(self.calls, [])
        self.assertIn("Authenticode", self.printed(err))

    def test_an_installer_changed_while_it_is_signed_aborts_and_the_signature_goes(self):
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.run_sign(changes_installer=True)
        self.assertEqual(len(self.calls), 1)
        self.assertFalse(self.setup.with_name(self.setup.name + ".sig").exists())

    def test_the_hash_is_the_one_from_the_authenticode_check(self):
        expected = release.sha256_hex(self.setup)
        _, signed = self.run_sign(expected_sha256=expected)
        self.assertEqual(signed.sha256, expected)

    def test_a_signature_of_another_key_aborts_and_is_removed(self):
        _, foreign = throwaway_key_pair(self.folder, bytes.fromhex("B1B2B3B4B5B6B7B8"))
        with self.assertRaises(SystemExit):
            self.run_sign(signature=foreign)
        self.assertFalse(self.setup.with_name(self.setup.name + ".sig").exists())


class VerifierBuildTest(unittest.TestCase):
    """build_verifier(): compiled before `tauri build`, path taken from Cargo's own report."""

    def setUp(self):
        self.folder = test_folder(self, "release verifier ")
        self.binary = self.folder / "verify_update_sig.exe"
        self.binary.write_bytes(b"verifier-marker")

    def run_build(self, messages, returncode=0, environ=None):
        calls = []

        def fake_run(command, **kwargs):
            calls.append((command, kwargs.get("env")))
            stdout = "".join(json.dumps(message) + "\n" for message in messages)
            return subprocess.CompletedProcess(command, returncode, stdout=stdout)

        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release.subprocess, "run", side_effect=fake_run
        ), mock.patch.dict(release.os.environ, dict(os.environ, **(environ or {})), clear=True):
            verifier = release.build_verifier()
        return calls, verifier

    def artifact(self, name="verify_update_sig", kind="example", executable=None):
        return {
            "reason": "compiler-artifact",
            "target": {"name": name, "kind": [kind]},
            "executable": str(executable or self.binary),
        }

    def test_builds_the_example_offline_from_the_lock_file_without_the_key(self):
        calls, verifier = self.run_build(
            [{"reason": "build-script-executed"}, self.artifact("hangar", "lib"), self.artifact()],
            environ=SIGNING_ENVIRONMENT,
        )
        command, env = calls[0]
        self.assertEqual(command[:2], [CARGO, "build"])
        for flag in ("--locked", "--offline", "--message-format=json"):
            self.assertIn(flag, command)
        self.assertEqual(command[command.index("--example") + 1], "verify_update_sig")
        self.assertEqual(command[command.index("--manifest-path") + 1], str(release.CARGO_TOML))
        self.assertEqual([name for name in env if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], [])
        self.assertEqual(verifier, release.Verifier(self.binary, release.sha256_hex(self.binary)))

    def test_a_failed_build_or_missing_binary_aborts(self):
        for messages, code in (
            ([self.artifact()], 101),
            ([self.artifact("hangar", "lib")], 0),
            ([self.artifact(executable=self.folder / "missing.exe")], 0),
        ):
            with self.subTest(code=code), mock.patch("sys.stderr"), self.assertRaises(SystemExit):
                self.run_build(messages, returncode=code)


class CollectTest(unittest.TestCase):
    """collect(): the copies under release/ must be the bytes that were signed."""

    def setUp(self):
        self.folder = test_folder(self, "release collect ")
        self.setup = self.folder / "Hangar_0.2.0+31_x64-setup.exe"
        self.setup.write_bytes(b"signed installer")
        signature = self.setup.with_name(self.setup.name + ".sig")
        signature.write_text("signature-marker", encoding="utf-8")
        self.signed = release.Signed(signature, "signature-marker", release.sha256_hex(self.setup))
        verifier = self.folder / "verify update sig.exe"
        verifier.write_bytes(b"verifier-marker")
        self.verifier = release.Verifier(verifier, release.sha256_hex(verifier))
        patch = mock.patch.object(release, "RELEASE_DIR", self.folder / "release")
        patch.start()
        self.addCleanup(patch.stop)

    def collect(self, verify_returncode=0, environ=None):
        # Each verifier run: its command, environment, and the installer and signature text it
        # was handed, read while it runs.
        self.verified = []

        def fake_run(command, **kwargs):
            self.verified.append(
                (
                    command,
                    kwargs.get("env"),
                    Path(command[1]).read_bytes(),
                    Path(command[2]).read_text(encoding="utf-8"),
                )
            )
            return subprocess.CompletedProcess(command, verify_returncode)

        with mock.patch.object(release.subprocess, "run", side_effect=fake_run), mock.patch.object(
            release, "updater_pubkey", return_value="pubkey-marker"
        ), mock.patch.dict(release.os.environ, dict(os.environ, **(environ or {})), clear=True):
            return release.collect(
                self.setup, self.signed, "0.2.0+31", "https://example.org/dl", "notes", self.verifier
            )

    def assert_nothing_collected(self):
        target = self.folder / "release" / "0.2.0-31"
        for name in (
            "Hangar_0.2.0-31_x64-setup.exe",
            "Hangar-Setup.exe",
            "SHA256SUMS.txt",
            "latest.json",
            release.VERIFY_SIGNATURE_NAME,
        ):
            self.assertFalse((target / name).exists(), name)

    def test_writes_the_signed_installer_and_manifest(self):
        target, setup_name = self.collect()
        self.assertEqual(setup_name, "Hangar_0.2.0-31_x64-setup.exe")
        self.assertEqual((target / setup_name).read_bytes(), b"signed installer")
        manifest = json.loads((target / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["platforms"]["windows-x86_64"]["signature"], "signature-marker")

    def test_a_signature_file_changed_after_signing_does_not_count(self):
        self.signed.signature.write_text("replaced-marker", encoding="utf-8")
        target, _ = self.collect()
        self.assertNotIn("replaced-marker", (target / "latest.json").read_text(encoding="utf-8"))
        # The verifier saw the text latest.json carries, never the `.sig` again.
        _, _, _, text = self.verified[0]
        self.assertEqual(text, "signature-marker")

    def test_the_uploaded_copy_is_verified_without_the_key(self):
        target, setup_name = self.collect(environ=SIGNING_ENVIRONMENT)
        self.assertEqual(len(self.verified), 1)
        command, env, installer, text = self.verified[0]
        # The verifier built before `tauri build`; nothing compiles between signing and collecting.
        self.assertEqual(command[0], str(self.verifier.path))
        self.assertEqual(command[1], str(target / setup_name))
        self.assertEqual(Path(command[2]).parent, target)
        self.assertEqual(command[3:], ["pubkey-marker", "0.2.0+31"])
        self.assertEqual(installer, b"signed installer")
        self.assertEqual(text, "signature-marker")
        self.assertEqual([name for name in env if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], [])
        # The text file goes right after.
        self.assertFalse((target / release.VERIFY_SIGNATURE_NAME).exists())

    def test_a_changed_verifier_is_never_run(self):
        self.verifier.path.write_bytes(b"replaced-verifier")
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.collect()
        self.assertEqual(self.verified, [])
        self.assert_nothing_collected()

    def test_a_signature_that_does_not_verify_aborts_and_is_removed(self):
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.collect(verify_returncode=1)
        self.assertFalse(self.signed.signature.exists())
        self.assert_nothing_collected()

    def test_an_installer_changed_after_signing_aborts(self):
        self.setup.write_bytes(b"changed installer")
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.collect()
        self.assertEqual(self.verified, [])
        self.assert_nothing_collected()


class BuildInputTest(unittest.TestCase):
    """Files Git does not see (ignored or excluded) that would still change the build."""

    def setUp(self):
        self.folder = test_folder(self, "release inputs ")
        self.parent = self.folder / "parent"
        self.root = self.parent / "hangar"
        (self.root / "src-tauri").mkdir(parents=True)
        (self.root / "src-tauri" / "tauri.conf.json").write_text("{}", encoding="utf-8")

    def problems(self):
        return release.build_input_problems(self.root, [self.parent, self.folder])

    def test_a_clean_layout_passes(self):
        self.assertEqual(self.problems(), [])

    def test_a_hidden_platform_config_aborts(self):
        planted = self.root / "src-tauri" / "tauri.windows.conf.json"
        planted.write_text('{"build": {"beforeBuildCommand": "marker"}}', encoding="utf-8")
        self.assertEqual(self.problems(), [planted])
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            release.check_build_inputs(self.root, [self.parent, self.folder])
        printed = "".join(str(call.args[0]) for call in err.write.call_args_list if call.args)
        self.assertIn("tauri.windows.conf.json", printed)

    def test_every_config_override_name_counts(self):
        names = [
            "Tauri.toml",
            "tauri.linux.toml",
            "tauri.conf.json5",
            "tauri.macos.conf.json5",
            "TAURI.Windows.conf.json",
        ]
        for name in names:
            (self.root / "src-tauri" / name).write_text("", encoding="utf-8")
        self.assertEqual(sorted(path.name for path in self.problems()), sorted(names))

    def test_cargo_and_toolchain_files_in_every_folder_above(self):
        planted = [
            self.root / "src-tauri" / ".cargo" / "config.toml",
            self.root / ".cargo" / "config",
            self.parent / ".cargo" / "config.toml",
            self.folder / "rust-toolchain.toml",
            self.root / "src-tauri" / "rust-toolchain",
            self.root / ".npmrc",
        ]
        for path in planted:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")
        self.assertEqual(sorted(self.problems()), sorted(planted))

    def test_an_npmrc_above_the_repository_is_not_a_build_input(self):
        (self.parent / ".npmrc").write_text("", encoding="utf-8")
        self.assertEqual(self.problems(), [])

    def test_a_tracked_cargo_config_passes(self):
        planted = self.root / ".cargo" / "config.toml"
        planted.parent.mkdir()
        planted.write_text("", encoding="utf-8")
        hidden = self.root / "src-tauri" / ".cargo" / "config.toml"
        hidden.parent.mkdir()
        hidden.write_text("", encoding="utf-8")
        # Tracked: part of the commit, reviewed like any file. Untracked: still aborts.
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            release.check_build_inputs(self.root, [self.parent, self.folder], tracked={planted})
        printed = "".join(str(call.args[0]) for call in err.write.call_args_list if call.args)
        self.assertIn(str(hidden), printed)
        self.assertNotIn(str(planted), printed)
        hidden.unlink()
        release.check_build_inputs(self.root, [self.parent, self.folder], tracked={planted})

    def test_tracked_inputs_come_from_git_ls_files(self):
        config = release.ROOT / ".cargo" / "config.toml"
        # Above the repository: git is not asked about it, and it never counts as tracked.
        outside = release.ROOT.parent / ".cargo" / "config.toml"
        listed = subprocess.CompletedProcess([], 0, stdout=".cargo/config.toml\0", stderr="")
        with mock.patch.object(release, "git_run", return_value=listed) as run:
            self.assertEqual(release.tracked_build_inputs([config, outside]), {config})
        self.assertEqual(run.call_args.args, ("ls-files", "-z", "--", ".cargo/config.toml"))
        with mock.patch.object(release, "git_run") as run:
            self.assertEqual(release.tracked_build_inputs([]), set())
        run.assert_not_called()

    def test_the_folders_above_default_to_every_parent(self):
        planted = self.folder / ".cargo" / "config.toml"
        planted.parent.mkdir()
        planted.write_text("", encoding="utf-8")
        self.assertIn(planted, release.build_input_problems(self.root))


class GitCommandTest(unittest.TestCase):
    """git(): no updater key, no repository fsmonitor, hooks or filters."""

    def run_git(self, *args, environ=None, config_output="", config_code=1):
        calls = []

        def fake_run(command, **kwargs):
            calls.append((command, kwargs))
            code = config_code if "config" in command else 0
            stdout = config_output if "config" in command else ""
            return subprocess.CompletedProcess(command, code, stdout=stdout, stderr="")

        with mock.patch.object(release, "resolve_tool", side_effect=fake_tool), mock.patch.object(
            release.subprocess, "run", side_effect=fake_run
        ), mock.patch.dict(release.os.environ, dict(os.environ, **(environ or {})), clear=True):
            if args:
                release.git(*args)
            else:
                release.working_tree_dirty()
        return calls

    def test_git_never_sees_the_updater_key(self):
        environ = dict(SIGNING_ENVIRONMENT, GIT_DIR="elsewhere-marker")
        ((_, kwargs),) = self.run_git("rev-list", "--count", "HEAD", environ=environ)
        env = kwargs["env"]
        self.assertIsNotNone(env)
        self.assertEqual([name for name in env if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], [])
        self.assertNotIn("inherited-key-marker", env.values())
        # GIT_DIR and friends would point git at another repository than the one checked.
        self.assertNotIn("elsewhere-marker", env.values())
        self.assertEqual(env.get("PATH"), os.environ.get("PATH"))

    def test_git_runs_without_fsmonitor_and_hooks(self):
        ((command, _),) = self.run_git("rev-list", "--count", "HEAD")
        program = fake_tool("git")
        self.assertEqual(command[0], program)
        options = command[: command.index("rev-list")]
        self.assertIn("core.fsmonitor=false", options)
        # A hooks folder below a regular file can never exist.
        self.assertIn("core.hooksPath=" + program, options)

    def test_the_status_check_switches_off_filters_and_submodules(self):
        output = "filter.lfs.clean\ngit-lfs clean -- %f\0filter.my.dotted.process\nmarker\0"
        calls = self.run_git(config_output=output, config_code=0)
        self.assertEqual(len(calls), 2)
        status = calls[1][0]
        options = status[: status.index("status")]
        for name in ("lfs", "my.dotted"):
            for setting in ("clean=", "smudge=", "process=", "required=false"):
                self.assertIn("filter.%s.%s" % (name, setting), options)
        self.assertIn("core.fsmonitor=false", options)
        self.assertIn("--no-optional-locks", options)
        arguments = status[status.index("status") :]
        for flag in ("--porcelain", "--ignore-submodules=all", "--untracked-files=all"):
            self.assertIn(flag, arguments)

    def test_a_filter_name_that_cannot_be_switched_off_aborts(self):
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.run_git(config_output="filter.a=b.clean\nmarker\0", config_code=0)

    def test_a_failed_config_read_aborts(self):
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.run_git(config_code=128)

    def test_the_status_check_runs_in_a_real_repository(self):
        if not on_path("git"):
            self.skipTest("git not installed")
        self.assertIsInstance(release.working_tree_dirty(), bool)


class VerifySignatureEnvironmentTest(unittest.TestCase):
    def test_powershell_never_sees_the_updater_key(self):
        calls = []

        def fake_run(command, **kwargs):
            calls.append(kwargs.get("env"))
            report = '{"Status":"Valid","Timestamped":true}'
            return subprocess.CompletedProcess(command, 0, stdout=report, stderr="")

        path = Path("with space") / "Hangar-Setup.exe"
        with mock.patch.object(release, "resolve_tool", return_value=r"C:\pwsh\pwsh.exe"), mock.patch.object(
            release.subprocess, "run", side_effect=fake_run
        ), mock.patch.dict(release.os.environ, dict(os.environ, **SIGNING_ENVIRONMENT), clear=True), mock.patch(
            "sys.stdout"
        ):
            release.verify_signature(path)
        env = calls[0]
        self.assertEqual([name for name in env if name.upper().startswith("TAURI_SIGNING_PRIVATE_KEY")], [])
        self.assertEqual(env["HANGAR_VERIFY_PATH"], str(path))
        self.assertEqual(env.get("PATH"), os.environ.get("PATH"))


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


GITHUB_DOWNLOAD = "https://github.com/feoren/hangar/releases/download/v0.2.0"
SETUP_NAME = "Hangar_0.2.0-31_x64-setup.exe"
APPIMAGE_NAME = "Hangar_0.2.0-31_amd64.AppImage"
DEB_NAME = "Hangar_0.2.0-31_amd64.deb"


class LinuxReleaseTest(unittest.TestCase):
    """--linux: the CI bundles join a finished Windows release under release/0.2.0-31/."""

    def setUp(self):
        self.folder = test_folder(self, "release linux ")
        patch = mock.patch.object(release, "RELEASE_DIR", self.folder / "release")
        patch.start()
        self.addCleanup(patch.stop)
        self.target = self.folder / "release" / "0.2.0-31"
        self.target.mkdir(parents=True)
        self.write_windows_release(GITHUB_DOWNLOAD)
        # The layout of the artifact: upload-artifact keeps the folders below bundle/.
        self.bundles = self.folder / "linux bundles"
        self.write_bundles(self.bundles)
        verifier = self.folder / "verify update sig.exe"
        verifier.write_bytes(b"verifier-marker")
        self.verifier = release.Verifier(verifier, release.sha256_hex(verifier))

    def write_windows_release(self, base_url):
        (self.target / SETUP_NAME).write_bytes(b"windows installer")
        (self.target / "Hangar-Setup.exe").write_bytes(b"windows installer")
        self.manifest = {
            "version": "0.2.0+31",
            "notes": "- Neu",
            "pub_date": "2026-11-18T10:00:00+00:00",
            "platforms": {
                "windows-x86_64": {"signature": "windows-signature-marker", "url": "%s/%s" % (base_url, SETUP_NAME)}
            },
        }
        release.write_json(self.target / "latest.json", self.manifest)

    def write_bundles(self, folder, version="0.2.0+31"):
        (folder / "appimage").mkdir(parents=True, exist_ok=True)
        (folder / "deb").mkdir(exist_ok=True)
        (folder / "appimage" / ("Hangar_%s_amd64.AppImage" % version)).write_bytes(b"appimage")
        (folder / "deb" / ("Hangar_%s_amd64.deb" % version)).write_bytes(b"deb package")

    def signed_appimage(self, bundles):
        signature = bundles.appimage.with_name(bundles.appimage.name + ".sig")
        signature.write_text("linux-signature-marker", encoding="utf-8")
        return release.Signed(signature, "linux-signature-marker", release.sha256_hex(bundles.appimage))

    def collect(self, verify_returncode=0):
        self.verified = []

        def fake_run(command, **kwargs):
            self.verified.append((command, Path(command[1]).read_bytes(), Path(command[2]).read_text(encoding="utf-8")))
            return subprocess.CompletedProcess(command, verify_returncode)

        bundles = release.find_linux_bundles(self.bundles, "0.2.0+31")
        signed = self.signed_appimage(bundles)
        with mock.patch.object(release.subprocess, "run", side_effect=fake_run), mock.patch.object(
            release, "updater_pubkey", return_value="pubkey-marker"
        ):
            return release.collect_linux(bundles, signed, "0.2.0+31", self.target, self.verifier)

    def manifest_now(self):
        return json.loads((self.target / "latest.json").read_text(encoding="utf-8"))

    def test_finds_both_bundles_at_any_depth(self):
        bundles = release.find_linux_bundles(self.bundles, "0.2.0+31")
        self.assertEqual(bundles.appimage, self.bundles / "appimage" / "Hangar_0.2.0+31_amd64.AppImage")
        self.assertEqual(bundles.deb, self.bundles / "deb" / "Hangar_0.2.0+31_amd64.deb")
        self.assertEqual(bundles.deb_sha256, release.sha256_hex(bundles.deb))

    def test_the_shown_version_in_the_name_counts_too(self):
        other = self.folder / "renamed"
        self.write_bundles(other, "0.2.0-31")
        self.assertIsNotNone(release.find_linux_bundles(other, "0.2.0+31"))

    def test_no_bundle_at_all_means_none(self):
        empty = self.folder / "empty"
        empty.mkdir()
        self.assertIsNone(release.find_linux_bundles(empty, "0.2.0+31"))
        self.assertIsNone(release.find_linux_bundles(self.folder / "missing", "0.2.0+31"))

    def test_a_bundle_of_another_version_aborts(self):
        other = self.folder / "other"
        self.write_bundles(other, "0.1.9+30")
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            release.find_linux_bundles(other, "0.2.0+31")
        self.assertIn("not a bundle of version 0.2.0+31", "".join(str(c.args[0]) for c in err.write.call_args_list))

    def test_two_appimages_or_a_missing_deb_abort(self):
        (self.bundles / "Hangar_0.2.0+31_amd64.AppImage").write_bytes(b"second")
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            release.find_linux_bundles(self.bundles, "0.2.0+31")
        lonely = self.folder / "lonely"
        lonely.mkdir()
        (lonely / "Hangar_0.2.0+31_amd64.AppImage").write_bytes(b"appimage")
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            release.find_linux_bundles(lonely, "0.2.0+31")

    def test_latest_json_announces_both_platforms(self):
        setup_name, linux_names = self.collect()
        self.assertEqual(setup_name, SETUP_NAME)
        self.assertEqual(linux_names, [APPIMAGE_NAME, DEB_NAME])
        manifest = self.manifest_now()
        self.assertEqual(manifest["platforms"]["windows-x86_64"], self.manifest["platforms"]["windows-x86_64"])
        self.assertEqual(
            manifest["platforms"]["linux-x86_64-appimage"],
            {"signature": "linux-signature-marker", "url": "%s/%s" % (GITHUB_DOWNLOAD, APPIMAGE_NAME)},
        )
        # A plain linux-x86_64 would reach .deb installations, which cannot install an AppImage.
        self.assertNotIn("linux-x86_64", manifest["platforms"])
        for key in ("version", "notes", "pub_date"):
            self.assertEqual(manifest[key], self.manifest[key])
        self.assertEqual((self.target / APPIMAGE_NAME).read_bytes(), b"appimage")
        self.assertEqual((self.target / DEB_NAME).read_bytes(), b"deb package")

    def test_the_uploaded_appimage_copy_is_verified(self):
        self.collect()
        self.assertEqual(len(self.verified), 1)
        command, content, text = self.verified[0]
        self.assertEqual(command[0], str(self.verifier.path))
        self.assertEqual(command[1], str(self.target / APPIMAGE_NAME))
        self.assertEqual(command[3:], ["pubkey-marker", "0.2.0+31"])
        self.assertEqual((content, text), (b"appimage", "linux-signature-marker"))
        self.assertFalse((self.target / release.VERIFY_SIGNATURE_NAME).exists())

    def test_checksums_cover_installers_and_linux_bundles(self):
        self.collect()
        content = (self.target / "SHA256SUMS.txt").read_bytes()
        names = [line.split(b"  ", 1)[1].decode() for line in content.splitlines()]
        self.assertEqual(names, [SETUP_NAME, "Hangar-Setup.exe", APPIMAGE_NAME, DEB_NAME])
        self.assertIn(("%s  %s\n" % (release.sha256_hex(self.target / DEB_NAME), DEB_NAME)).encode(), content)
        self.assertNotIn(b"\r", content)

    def test_a_self_hosted_release_keeps_its_folder(self):
        self.write_windows_release("https://example.org/dl")
        self.collect()
        url = self.manifest_now()["platforms"]["linux-x86_64-appimage"]["url"]
        self.assertEqual(url, "https://example.org/dl/" + APPIMAGE_NAME)

    def test_a_failed_verification_leaves_the_release_as_it_was(self):
        before = (self.target / "latest.json").read_bytes()
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            self.collect(verify_returncode=1)
        self.assertEqual((self.target / "latest.json").read_bytes(), before)
        for name in (APPIMAGE_NAME, DEB_NAME, "SHA256SUMS.txt"):
            self.assertFalse((self.target / name).exists(), name)

    def test_a_deb_changed_after_it_was_found_aborts(self):
        bundles = release.find_linux_bundles(self.bundles, "0.2.0+31")
        signed = self.signed_appimage(bundles)
        bundles.deb.write_bytes(b"swapped package")
        with mock.patch("sys.stderr"), self.assertRaises(SystemExit):
            release.collect_linux(bundles, signed, "0.2.0+31", self.target, self.verifier)
        self.assertFalse((self.target / DEB_NAME).exists())
        self.assertNotIn("linux-x86_64-appimage", self.manifest_now()["platforms"])

    def test_without_a_windows_release_nothing_is_added(self):
        (self.target / "latest.json").unlink()
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            self.collect()
        self.assertIn("run --version first", "".join(str(c.args[0]) for c in err.write.call_args_list))

    def test_gh_commands_attach_the_linux_files_before_the_manifest(self):
        notes = self.target / "release-notes.md"
        _, create, _ = release.github_release_commands(
            "0.2.0", "0.2.0+31", self.target, SETUP_NAME, notes, [APPIMAGE_NAME, DEB_NAME]
        )
        files = create[4 : create.index("--repo")]
        self.assertEqual(
            files,
            [str(self.target / name) for name in (SETUP_NAME, "Hangar-Setup.exe", APPIMAGE_NAME, DEB_NAME, "SHA256SUMS.txt", "latest.json")],
        )
        self.assertIn("--draft", create)

    def test_publish_lines_name_every_file(self):
        lines = release.publish_lines("0.2.0", "0.2.0+31", self.target, SETUP_NAME, [APPIMAGE_NAME, DEB_NAME])
        create = next(line for line in lines if "gh release create" in line)
        self.assertLess(create.index(DEB_NAME), create.index("latest.json"))
        self.assertIn(APPIMAGE_NAME, create)
        self.assertTrue(any("gh release edit v0.2.0" in line for line in lines))
        self.write_windows_release("https://example.org/dl")
        lines = release.publish_lines("0.2.0", "0.2.0+31", self.target, SETUP_NAME, [APPIMAGE_NAME, DEB_NAME])
        self.assertIn("https://example.org/dl/", lines[0])
        self.assertEqual(lines[-1], "  6. latest.json")
        self.assertEqual(lines[3:5], ["  3. " + APPIMAGE_NAME, "  4. " + DEB_NAME])

    def test_the_windows_run_says_it_is_without_linux(self):
        lines = release.without_linux_lines("0.2.0")
        self.assertTrue(lines[0].startswith("Without Linux"))
        self.assertIn("  py -3 tools/release.py --linux 0.2.0", lines)

    def run_add_linux(self, bundles_dir, version="0.2.0+31", runs=None, download=None):
        """add_linux() with the build, signer, verifier, git and gh replaced; returns the gh calls
        and what was printed."""
        gh_calls = []

        def fake_gh(*args):
            gh_calls.append(args)
            if args[:2] == ("run", "list"):
                return json.dumps(runs or [])
            if download:
                download(Path(args[args.index("--dir") + 1]))
            return ""

        def fake_sign(key_path, setup, signed_version, expected_sha256, allow_unencrypted_key=False):
            self.assertEqual(signed_version, version)
            self.assertEqual(expected_sha256, release.sha256_hex(setup))
            signature = setup.with_name(setup.name + ".sig")
            signature.write_text("linux-signature-marker", encoding="utf-8")
            return release.Signed(signature, "linux-signature-marker", expected_sha256)

        out = io.StringIO()
        with mock.patch.object(release, "current_version", return_value=version), mock.patch.object(
            release, "gh_run", side_effect=fake_gh
        ), mock.patch.object(release, "git", return_value="c0ffee" * 6 + "abcd"), mock.patch.object(
            release, "working_tree_dirty", return_value=False
        ), mock.patch.object(release, "build_verifier", return_value=self.verifier), mock.patch.object(
            release, "sign_update", side_effect=fake_sign
        ), mock.patch.object(release, "update_signature_problem", return_value=None), contextlib.redirect_stdout(out):
            release.add_linux("0.2.0", bundles_dir, self.folder / "test key.key", False, False)
        return gh_calls, out.getvalue()

    def test_add_linux_from_a_folder(self):
        gh_calls, printed = self.run_add_linux(self.bundles)
        self.assertEqual(gh_calls, [])
        self.assertIn("linux-x86_64-appimage", self.manifest_now()["platforms"])
        self.assertIn(APPIMAGE_NAME, printed)
        self.assertIn("gh release create v0.2.0", printed)
        # A folder the user named is never removed.
        self.assertTrue(self.bundles.is_dir())

    def test_add_linux_downloads_the_push_run_of_the_commit(self):
        runs = [
            {"databaseId": 9, "status": "completed", "conclusion": "success", "event": "pull_request", "headBranch": "main"},
            {"databaseId": 7, "status": "completed", "conclusion": "failure", "event": "push", "headBranch": "main"},
        ]
        gh_calls, _ = self.run_add_linux(None, runs=runs, download=self.write_bundles)
        listed, downloaded = gh_calls
        self.assertEqual(listed[listed.index("--commit") + 1], "c0ffee" * 6 + "abcd")
        self.assertEqual(listed[listed.index("--workflow") + 1], "ci.yml")
        self.assertEqual(downloaded[:3], ("run", "download", "7"))
        self.assertEqual(downloaded[downloaded.index("--name") + 1], "linux-bundles")
        self.assertIn("linux-x86_64-appimage", self.manifest_now()["platforms"])
        self.assertFalse((self.target / release.LINUX_DOWNLOAD_NAME).exists())

    def test_without_linux_bundles_the_release_stays_as_it_was(self):
        empty = self.folder / "empty"
        empty.mkdir()
        before = (self.target / "latest.json").read_bytes()
        with mock.patch("sys.stderr") as err, self.assertRaises(SystemExit):
            self.run_add_linux(empty)
        self.assertIn("stays without Linux", "".join(str(c.args[0]) for c in err.write.call_args_list))
        self.assertEqual((self.target / "latest.json").read_bytes(), before)

    def test_add_linux_needs_the_release_state_in_the_files(self):
        for version in ("0.3.0", "0.2.0", "0.1.0+30"):
            with self.subTest(version=version), mock.patch("sys.stderr"), self.assertRaises(SystemExit):
                self.run_add_linux(self.bundles, version=version)
        self.assertNotIn("linux-x86_64-appimage", self.manifest_now()["platforms"])


class CiRunTest(unittest.TestCase):
    COMMIT = "0123456789abcdef0123456789abcdef01234567"

    def pick(self, runs):
        return release.pick_ci_run(json.dumps(runs), self.COMMIT)

    def test_the_newest_push_run_on_main_counts(self):
        runs = [
            {"databaseId": 3, "status": "completed", "event": "pull_request", "headBranch": "main"},
            {"databaseId": 2, "status": "completed", "event": "push", "headBranch": "main"},
            {"databaseId": 1, "status": "completed", "event": "push", "headBranch": "main"},
        ]
        self.assertEqual(self.pick(runs), (2, None))

    def test_no_run_or_an_unfinished_run_is_explained(self):
        self.assertIn("push the release commit", self.pick([])[1])
        self.assertIn("0123456789ab", self.pick([])[1])
        running = [{"databaseId": 5, "status": "in_progress", "event": "push", "headBranch": "main"}]
        self.assertEqual(self.pick(running)[0], None)
        self.assertIn("not finished", self.pick(running)[1])

    def test_unreadable_output_is_refused(self):
        for text in ("", "{}", "[1]", json.dumps([{"databaseId": "7", "status": "completed", "event": "push", "headBranch": "main"}])):
            with self.subTest(text=text):
                run_id, problem = release.pick_ci_run(text, self.COMMIT)
                self.assertIsNone(run_id)
                self.assertIsNotNone(problem)


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
