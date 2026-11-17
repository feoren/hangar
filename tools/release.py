"""Builds a Hangar release: a signed Windows installer plus latest.json for the auto-updater.

Usage:
  py -3 tools/release.py --version 0.2.0                  # notes from src/changelog.json
  py -3 tools/release.py --version 0.2.0 --notes "What's new"
  py -3 tools/release.py --version 0.2.0 --no-build      # only set the version, do not build
  py -3 tools/release.py --next 0.3.0                    # after the release: create the upcoming version
  py -3 tools/release.py --version 0.2.0 --base-url https://example.org/download   # self-hosted

Rule "the code always carries the upcoming version" (ADR 0002): tauri.conf.json, package.json and
Cargo.toml name the version that ships next, and it is the highest one in src/changelog.json (still
without a date there). So --version must equal the highest changelog version, must not be lower than
the version in the files, and must be higher than the last published one (the highest dated
changelog entry or folder under release/). After the release, --next X.Y.Z moves the files to the
upcoming version and adds it to the changelog, empty and undated.

Notes: without --notes, the script uses the German items of this version's changelog entry
(src/changelog.json, the same source as "What's New" in the app) as a list. If the entry is missing
or has no German items, it aborts, even with --notes. If the entry has no date yet ("date": null
means in preparation), it sets today's date (also with --notes) so the app shows the version as
released; commit the file afterwards.

Version scheme (docs/wiki/pages/decisions/0002-version-scheme.md, following de.wikipedia.org/wiki/Versionsnummer):
  Major.Minor.Revision-Build, e.g. 0.12.224-1548. The build number is buildNumberBase from
  package.json plus the number of commits at the current state (`git rev-list --count HEAD`) and is
  never reset.
  Internally it is SemVer build metadata after "+" (0.12.224+1548), from which Tauri derives the
  Windows file version 0.12.224.1548. The updater ignores build metadata when comparing versions,
  so every release must raise the major, minor or revision number.

1. Sets the version in src-tauri/tauri.conf.json, src-tauri/Cargo.toml and package.json.
2. Refuses to build when a file Git does not see could change the build: a Tauri config next to
   tauri.conf.json (tauri.<platform>.conf.json, Tauri.toml, tauri.conf.json5), a Cargo config or
   rust-toolchain file in src-tauri, the repository or any folder above it, or an .npmrc in the
   repository. The clean tree check alone misses them when they are ignored or excluded.
3. Builds the verifier (`cargo build --example verify_update_sig`), then the app with `tauri build`
   (the CLI's tauri.js run by node.exe directly, never through the pnpm.cmd shim, so no argument
   passes cmd.exe), with updater signing switched off and without the updater key in its
   environment (no build script or dependency ever sees it). Afterwards a separate
   `tauri signer sign --app-version <version>` signs the finished installer, the only process
   that gets the private key (default: %USERPROFILE%\\.tauri\\hangar.key, override with
   HANGAR_SIGNING_KEY) and its password, with a reduced environment (no NODE_OPTIONS or other
   Node settings). The script asks for the password at that point; it never takes it from the
   environment and refuses to run while TAURI_SIGNING_PRIVATE_KEY_PASSWORD is set there. A key
   without a password is used only with --allow-unencrypted-key. The signature is the same `.sig`
   next to the installer that `tauri build` writes, with the version in its trusted comment. Its
   key id must match the public key in tauri.conf.json; otherwise the script aborts. The
   installer's SHA-256 is taken right after the Authenticode check (or, unsigned, right before the
   updater signer) and must still hold before and after the signer, and the copies under release/
   must have it. The verifier built before the build (no key in its environment) then checks the
   copy under release/ the way installed apps do, against the signature text latest.json gets
   (from a file of its own, not the `.sig` again).
   git, node, cargo and PowerShell are taken only as absolute programs from PATH (on Windows only
   `.exe`), never from the repository or the working folder. git runs without the repository's
   fsmonitor, hooks and filters, and none of these programs sees an updater key variable.
4. Puts everything to upload under release/<version>/:
     Hangar_<x.y.z-build>_x64-setup.exe   installer for this version (target of the update URL)
     Hangar-Setup.exe                 same file under a fixed name for the download link
     SHA256SUMS.txt                   SHA-256 checksums of both installers (sha256sum format, LF)
     latest.json                      polled by installed apps
5. Publish. By default the files go to a GitHub release with the tag vX.Y.Z; the script prints the
   `gh` commands for it (it never publishes by itself). The release is created as a draft, the files
   are attached, and only then is it published, so .../releases/latest/download/latest.json never
   announces a version whose installer is missing. With --base-url, upload to that folder instead:
   installers first, then SHA256SUMS.txt, and latest.json last.
   Users verify a download with `sha256sum -c SHA256SUMS.txt --ignore-missing` or by comparing
   `Get-FileHash Hangar-Setup.exe` with the line for that file.

Authenticode code signing of the installer (against the SmartScreen warning), optional via environment:
  HANGAR_CERT_THUMBPRINT   SHA-1 thumbprint of a certificate in the Windows certificate store;
                           Tauri then signs with signtool (SHA-256, timestamped).
  HANGAR_TIMESTAMP_URL     timestamp server for it (default http://time.certum.pl).
  HANGAR_TIMESTAMP_TSP     1 (default) = RFC 3161 (signtool /tr), 0 = legacy Authenticode (/t).
  HANGAR_SIGN_COMMAND      alternatively, a custom signing command as a JSON argv with "%1" for the file,
                           e.g. ["signtool.exe","sign","/sha1","<thumbprint>","/fd","sha256",
                           "/tr","http://time.certum.pl","/td","sha256","%1"].
  Without these variables the script builds unsigned as before and says so. With them, the setting
  goes to `tauri build` as a --config file (tauri.conf.json stays unchanged), and after the
  build Get-AuthenticodeSignature checks the installer and the program: without a valid, timestamped
  signature the script aborts. Process and vendors: docs/wiki/pages/syntheses/release-rollout.md.

No dependencies beyond Python 3 and the project toolchain. Never prints keys, thumbprints or
signing commands.
"""
import argparse
import base64
import collections
import datetime
import getpass
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"
CARGO_TOML = ROOT / "src-tauri" / "Cargo.toml"
CARGO_LOCK = ROOT / "src-tauri" / "Cargo.lock"
PACKAGE_JSON = ROOT / "package.json"
CHANGELOG_JSON = ROOT / "src" / "changelog.json"
BUNDLE_DIR = ROOT / "src-tauri" / "target" / "release" / "bundle" / "nsis"
MAIN_EXE = ROOT / "src-tauri" / "target" / "release" / "hangar.exe"
RELEASE_DIR = ROOT / "release"
CHECKSUMS_NAME = "SHA256SUMS.txt"
PNPM_LOCK = ROOT / "pnpm-lock.yaml"
GITHUB_REPO = "feoren/hangar"
# Older installations only poll this server; it gets a copy of latest.json (see release-rollout.md).
LEGACY_UPDATE_URL = "https://hangar.feoren.com/download/"
DEFAULT_KEY = Path.home() / ".tauri" / "hangar.key"
# The Tauri CLI's own entry point; run with node.exe, not through a .cmd shim.
TAURI_JS = ROOT / "node_modules" / "@tauri-apps" / "cli" / "tauri.js"
TAURI_CLI_PACKAGE = TAURI_JS.with_name("package.json")
BASE_VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
FULL_VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:\+(\d+))?$")
DEFAULT_TIMESTAMP_URL = "http://time.certum.pl"
THUMBPRINT = re.compile(r"^[0-9A-F]{40}$")
TIMESTAMP_URL = re.compile(r"^https?://\S+$")


class SigningError(ValueError):
    """Invalid signing setting. The message never names the value, only the variable."""


def windows_signing_config(env):
    """Builds the --config override for `tauri build` from the environment variables.

    Returns None when nothing is set (unsigned, same behavior as before). Pure function: it reads
    only the mapping it is given, so it can be tested without a build.
    """
    thumbprint = env.get("HANGAR_CERT_THUMBPRINT", "").strip()
    command = env.get("HANGAR_SIGN_COMMAND", "").strip()
    if thumbprint and command:
        raise SigningError("HANGAR_CERT_THUMBPRINT and HANGAR_SIGN_COMMAND exclude each other; set only one.")
    if command:
        return {"bundle": {"windows": {"signCommand": _sign_command(command)}}}
    if not thumbprint:
        return None

    # Thumbprints copied from the certificate viewer often contain spaces, colons or an invisible
    # direction mark (U+200E) at the start; none of that is part of the value.
    thumbprint = re.sub(r"[\s:\u200e\u200f]", "", thumbprint).upper()
    if not THUMBPRINT.match(thumbprint):
        raise SigningError("HANGAR_CERT_THUMBPRINT is not a SHA-1 thumbprint (40 hex characters).")

    # Without a timestamp the signature becomes invalid once the certificate expires, so it is required.
    timestamp_url = env.get("HANGAR_TIMESTAMP_URL", "").strip() or DEFAULT_TIMESTAMP_URL
    if not TIMESTAMP_URL.match(timestamp_url):
        raise SigningError(
            "HANGAR_TIMESTAMP_URL must start with http:// or https:// and must not contain spaces."
        )
    tsp = env.get("HANGAR_TIMESTAMP_TSP", "").strip() or "1"
    if tsp not in ("0", "1"):
        raise SigningError("HANGAR_TIMESTAMP_TSP expects 0 or 1.")
    return {
        "bundle": {
            "windows": {
                "certificateThumbprint": thumbprint,
                "digestAlgorithm": "sha256",
                "timestampUrl": timestamp_url,
                # RFC 3161 (signtool /tr /td) yields SHA-256 timestamps; the legacy /t only SHA-1.
                "tsp": tsp == "1",
            }
        }
    }


def _sign_command(raw):
    """HANGAR_SIGN_COMMAND as argv. Tauri's string form splits on spaces and breaks paths like
    "C:\\Program Files", so only a JSON array is accepted, passed on as {cmd, args}."""
    try:
        argv = json.loads(raw)
    except ValueError:
        raise SigningError('HANGAR_SIGN_COMMAND must be a JSON array, e.g. ["signtool.exe", "sign", …, "%1"].') from None
    if not isinstance(argv, list) or len(argv) < 2 or not all(isinstance(part, str) and part for part in argv):
        raise SigningError("HANGAR_SIGN_COMMAND must be a JSON array of at least two non-empty strings.")
    if not any("%1" in part for part in argv[1:]):
        raise SigningError('HANGAR_SIGN_COMMAND needs an argument containing "%1" (placeholder for the file).')
    return {"cmd": argv[0], "args": argv[1:]}


# Prints only the status and whether a timestamp is present. The path comes in through the
# environment so it never has to be spliced into PowerShell source and quoted.
VERIFY_SCRIPT = (
    "$s = Get-AuthenticodeSignature -LiteralPath $env:HANGAR_VERIFY_PATH; "
    "[pscustomobject]@{ Status = [string]$s.Status; "
    "Timestamped = [bool]$s.TimeStamperCertificate } | ConvertTo-Json -Compress"
)


def signature_problem(report_text):
    """Evaluates the JSON output of VERIFY_SCRIPT: None for a valid, timestamped signature,
    otherwise the reason."""
    try:
        report = json.loads(report_text)
    except ValueError:
        return "output of Get-AuthenticodeSignature is not readable"
    if not isinstance(report, dict):
        return "output of Get-AuthenticodeSignature is not readable"
    status = report.get("Status")
    if status != "Valid":
        return "signature status %s instead of Valid" % (status if isinstance(status, str) else "unknown")
    if report.get("Timestamped") is not True:
        return "signature without timestamp (becomes invalid when the certificate expires)"
    return None


def verify_signature(path):
    shell = resolve_tool("pwsh", "powershell")
    env = without_updater_key(os.environ)
    env["HANGAR_VERIFY_PATH"] = str(path)
    result = subprocess.run(
        [shell, "-NoProfile", "-NonInteractive", "-Command", VERIFY_SCRIPT],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        fail("Get-AuthenticodeSignature failed for %s: %s" % (path.name, result.stderr.strip()))
    problem = signature_problem(result.stdout.strip())
    if problem:
        fail("%s is not validly signed: %s" % (path.name, problem))
    print("Signature valid, with timestamp: %s" % path.name)


def fail(message):
    print("ERROR: " + message, file=sys.stderr)
    sys.exit(1)


def is_inside(path, folder):
    """Whether `path` is `folder` or lies below it (Windows: ignoring case)."""
    inner = os.path.normcase(os.path.abspath(path))
    outer = os.path.normcase(os.path.abspath(folder))
    try:
        return os.path.commonpath([inner, outer]) == outer
    except ValueError:
        # Different drives.
        return False


def find_program(name, search_path, excluded, windows, working_folder=None):
    """The first `name` in an absolute folder of `search_path` outside every `excluded` folder.

    Windows looks for programs in the working folder before PATH (CreateProcess and, depending on
    the Python version, shutil.which) and prefers `.com` over `.exe` (PATHEXT); a file planted in
    the repository could then run instead of git or node and read the signing key. So only
    `<name>.exe` counts there (a .cmd or .bat would also run through cmd.exe), relative PATH
    entries are skipped because they resolve against the working folder, folders inside
    `excluded` (the repository) never count, also not through a link, and neither does
    `working_folder` itself. Only that folder, not its subfolders: Windows searches nothing below
    it, and started from the home folder, ~/.cargo/bin would otherwise never be found.
    """
    program = name + ".exe" if windows else name
    excluded = [Path(folder).resolve() for folder in excluded]
    working = os.path.normcase(str(Path(working_folder).resolve())) if working_folder else None
    for entry in search_path.split(os.pathsep):
        entry = entry.strip().strip('"')
        if not entry or not os.path.isabs(entry):
            continue
        try:
            folder = Path(entry).resolve(strict=True)
        except OSError:
            continue
        candidate = folder / program
        if not candidate.is_file() or (not windows and not os.access(candidate, os.X_OK)):
            continue
        real = candidate.resolve()
        if any(is_inside(path, other) for path in (folder, real) for other in excluded):
            continue
        if working in (os.path.normcase(str(folder)), os.path.normcase(str(real.parent))):
            continue
        return str(candidate)
    return None


def resolve_tool(*names):
    """Absolute path of the first of `names` on PATH; aborts when none is there."""
    try:
        working_folder = Path.cwd()
    except OSError:
        working_folder = None
    for name in names:
        found = find_program(name, os.environ.get("PATH", ""), [ROOT], os.name == "nt", working_folder)
        if found:
            return found
    fail(
        "%s not found on PATH (programs in the repository or the working folder are never used)"
        % " or ".join(names)
    )


def read_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write_json(path, data):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write("\n")


def current_version():
    return read_json(TAURI_CONF)["version"]


def set_json_version(path, version):
    """Replaces only the value of the top-level "version" in the text. json.dump would reformat the
    file (Prettier keeps short lists on one line), and `pnpm format:check` would then fail."""
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(r'(?m)^(  "version":\s*")[^"]*(")', r"\g<1>" + version + r"\g<2>", text, count=1)
    if count != 1 or json.loads(updated).get("version") != version:
        fail("version not found in %s" % path.name)
    path.write_text(updated, encoding="utf-8", newline="\n")


def set_version(version):
    set_json_version(TAURI_CONF, version)
    set_json_version(PACKAGE_JSON, version)

    cargo = CARGO_TOML.read_text(encoding="utf-8")
    # Only the version in the [package] section, not those of the dependencies.
    updated, count = re.subn(
        r'(\[package\][^\[]*?\nversion\s*=\s*")[^"]+(")', r"\g<1>" + version + r"\g<2>", cargo, count=1
    )
    if count != 1:
        fail("version not found in Cargo.toml")
    CARGO_TOML.write_text(updated, encoding="utf-8", newline="\n")

    # Update Cargo.lock right away; otherwise only the next Cargo run changes the file and the
    # commit with the new version would be incomplete.
    if CARGO_LOCK.is_file():
        lock = CARGO_LOCK.read_text(encoding="utf-8")
        updated, count = re.subn(
            r'(\[\[package\]\]\nname = "hangar"\nversion = ")[^"]+(")', r"\g<1>" + version + r"\g<2>", lock, count=1
        )
        if count != 1:
            fail("package hangar not found in Cargo.lock")
        CARGO_LOCK.write_text(updated, encoding="utf-8", newline="\n")


def find_changelog_entry(version):
    """Changelog list and the entry for X.Y.Z (or None); aborts if the file is broken."""
    try:
        entries = read_json(CHANGELOG_JSON)
    except (OSError, ValueError) as error:
        fail("changelog %s is not readable: %s" % (CHANGELOG_JSON, error))
    if not isinstance(entries, list):
        fail("changelog %s is not a list" % CHANGELOG_JSON)
    for entry in entries:
        if isinstance(entry, dict) and entry.get("version") == version:
            return entries, entry
    return entries, None


def changelog_notes(entry):
    """German items as a list, one line per item."""
    items = entry.get("items")
    lines = []
    for item in items if isinstance(items, list) else []:
        text = item.get("de") if isinstance(item, dict) else None
        if isinstance(text, str) and text.strip():
            lines.append("- " + text.strip())
    if not lines:
        fail("changelog entry %s has no German items" % entry.get("version"))
    return "\n".join(lines)


def date_changelog(entries, entry):
    """Sets today's date while the entry is still in preparation (date: null)."""
    if entry.get("date") is not None:
        return
    entry["date"] = datetime.date.today().isoformat()
    write_json(CHANGELOG_JSON, entries)
    print("Changelog %s dated %s." % (entry["version"], entry["date"]))


def git_environment(environ):
    """The environment of every git call: no updater key variable and no GIT_* variable, through
    which GIT_DIR, GIT_INDEX_FILE or GIT_CONFIG_* would point git at another repository or config
    than the one checked (Windows names ignore case)."""
    return {
        name: value
        for name, value in without_updater_key(environ).items()
        if not name.upper().startswith("GIT_")
    }


def git_run(*args, allowed=(0,)):
    """Runs git in ROOT and returns the finished process; aborts on an exit code outside `allowed`.

    The repository's config can name programs git runs on its own: an fsmonitor for status, hooks
    for some commands. `core.fsmonitor=false` and a hooks path below a regular file (git itself,
    where no folder can ever exist) switch both off; the command line wins over every config file.
    """
    program = resolve_tool("git")
    command = [program, "--no-pager", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=" + program, *args]
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=git_environment(os.environ),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if result.returncode not in allowed:
        fail("git %s failed: %s" % (" ".join(args), result.stderr.strip()))
    return result


def git(*args):
    return git_run(*args).stdout.strip()


# Settings through which a repository makes git run a program on a file (clean, smudge, process).
FILTER_KEYS = r"^filter\..*\.(clean|smudge|process)$"


def filter_overrides(config_output):
    """`-c` arguments that switch off every filter driver `git config -z --get-regexp` listed:
    git skips a filter whose command is empty, and `required=false` keeps it from failing the call
    instead. None when a driver name cannot be passed back through `-c`, which splits at the first
    `=`."""
    names = set()
    for record in config_output.split("\0"):
        key = record.split("\n", 1)[0]
        if not key:
            continue
        name = key[len("filter.") :].rpartition(".")[0] if key.startswith("filter.") else ""
        if not name or "=" in name:
            return None
        names.add(name)
    overrides = []
    for name in sorted(names):
        for setting in ("clean=", "smudge=", "process=", "required=false"):
            overrides += ["-c", "filter.%s.%s" % (name, setting)]
    return overrides


def working_tree_dirty():
    """Whether `git status` reports any change, without running a program the repository names.

    `status` runs a clean filter on files whose stat data changed, so the configured drivers are
    read first (reading the config runs nothing) and switched off. Submodules are not entered
    (each has a config of its own), and untracked files are listed even when the repository's
    config hides them. Ignored and excluded files stay invisible here; check_build_inputs() covers
    the ones that change the build.
    """
    config = git_run("config", "-z", "--get-regexp", FILTER_KEYS, allowed=(0, 1))
    # 1: no such key, the normal case.
    overrides = filter_overrides(config.stdout if config.returncode == 0 else "")
    if overrides is None:
        fail("the repository configures a filter whose name git cannot switch off; check .git/config")
    status = git_run(
        *overrides,
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--ignore-submodules=all",
        "--untracked-files=all",
    )
    return bool(status.stdout.strip())


# Tauri merges these into tauri.conf.json (platform files, TOML and JSON5 forms); any of them can
# change the build commands or the bundle.
TAURI_CONFIG_OVERRIDE = re.compile(r"^tauri(\.[^.]+)?\.(conf\.json5?|toml)$", re.IGNORECASE)
# Read by Cargo and rustup in the build folder and every folder above it.
CARGO_INPUTS = (
    Path(".cargo") / "config",
    Path(".cargo") / "config.toml",
    Path("rust-toolchain"),
    Path("rust-toolchain.toml"),
)


def build_input_problems(root, ancestors=None):
    """Files that change the build although `git status` may not show them (ignored, excluded or
    outside the repository): Tauri config overrides next to tauri.conf.json, Cargo configs and
    toolchain files in src-tauri, the repository and every folder above it (`ancestors`, default
    all of them up to the drive root) and an .npmrc in the repository."""
    root = Path(root)
    tauri_dir = root / "src-tauri"
    found = []
    if tauri_dir.is_dir():
        found += sorted(
            path
            for path in tauri_dir.iterdir()
            if TAURI_CONFIG_OVERRIDE.match(path.name) and path.name != "tauri.conf.json"
        )
    if ancestors is None:
        ancestors = list(root.resolve().parents)
    for folder in [tauri_dir, root, *ancestors]:
        for name in CARGO_INPUTS:
            # lexists: a dangling link or a folder in its place counts as well.
            if os.path.lexists(folder / name):
                found.append(folder / name)
    if os.path.lexists(root / ".npmrc"):
        found.append(root / ".npmrc")
    return found


def tracked_build_inputs(paths):
    """The `paths` Git tracks in this repository. A tracked file is part of the commit and shows up
    in `git status` when changed, so it is no hidden build input: `.cargo/config.toml` sets the
    whisper.cpp build flags for every build, the release included."""
    inside = {}
    for path in paths:
        try:
            inside[Path(path).resolve().relative_to(ROOT).as_posix()] = path
        except ValueError:
            continue
    if not inside:
        return set()
    listed = git_run("ls-files", "-z", "--", *inside).stdout
    return {inside[name] for name in listed.split("\0") if name in inside}


def check_build_inputs(root=None, ancestors=None, tracked=None):
    """Aborts when build_input_problems() finds anything Git does not track (`tracked`, default
    tracked_build_inputs()); names the files, never their content."""
    found = build_input_problems(ROOT if root is None else root, ancestors)
    known = tracked_build_inputs(found) if tracked is None else tracked
    found = [path for path in found if path not in known]
    if found:
        fail(
            "these files would change the build but Git does not check them; move them away or "
            "release from a fresh clone (release-rollout.md):\n  %s" % "\n  ".join(str(path) for path in found)
        )


def build_number_from(package, commit_count):
    """buildNumberBase from package.json plus the commit count (ADR 0002).

    The base carries the count of the history before the public start, so build numbers never go
    down when the history was replaced.
    """
    base = package.get("buildNumberBase", 0)
    if not isinstance(base, int) or isinstance(base, bool) or base < 0:
        fail("buildNumberBase in package.json must be a whole number >= 0.")
    return base + commit_count


def build_number():
    return build_number_from(read_json(PACKAGE_JSON), int(git("rev-list", "--count", "HEAD")))


def parse_version(version):
    """(X, Y, Z) and whether a build number is present; None if the text does not fit the scheme."""
    match = FULL_VERSION.match(version) if isinstance(version, str) else None
    if not match:
        return None
    return tuple(int(part) for part in match.group(1, 2, 3)), match.group(4) is not None


def dotted(version_tuple):
    return ".".join(map(str, version_tuple))


def changelog_versions(entries):
    """(version as a tuple, date or None) for every readable entry."""
    found = []
    for entry in entries if isinstance(entries, list) else []:
        version = entry.get("version") if isinstance(entry, dict) else None
        if isinstance(version, str) and BASE_VERSION.match(version):
            found.append((tuple(int(part) for part in version.split(".")), entry.get("date")))
    return found


def last_published(entries, release_dir):
    """Highest released version: a dated changelog entry or a folder under release/.

    Both sources, because release/ is not in the repo (it is missing in fresh clones) and a
    changelog date only arrives with the commit after the release. Folders are named X.Y.Z-Build
    or X.Y.Z.
    """
    found = [version for version, date in changelog_versions(entries) if date is not None]
    if release_dir.is_dir():
        for child in release_dir.iterdir():
            parsed = parse_version(child.name.replace("-", "+", 1)) if child.is_dir() else None
            if parsed is not None:
                found.append(parsed[0])
    return max(found) if found else None


def release_problem(version, file_version, entries, release_dir):
    """Why --version X.Y.Z is not allowed, or None. Rule "the code always carries the upcoming
    version" (ADR 0002): the version is the highest in the changelog, not lower than the one in the
    files and higher than the last published one, because the updater compares only X.Y.Z."""
    if not BASE_VERSION.match(version):
        return "--version expects X.Y.Z, e.g. 0.12.224 (the script adds the build number)"
    new = tuple(int(part) for part in version.split("."))
    versions = changelog_versions(entries)
    if not versions:
        return "src/changelog.json has no entry; add the upcoming version there."
    highest = max(version_tuple for version_tuple, _ in versions)
    if new != highest:
        return "--version %s must be the highest version in src/changelog.json (%s)." % (version, dotted(highest))
    parsed = parse_version(file_version)
    if parsed is None:
        return "version '%s' in tauri.conf.json does not match the scheme X.Y.Z or X.Y.Z+Build" % file_version
    # The old rule was "higher than the files if they carry a build number". Now that the files
    # carry the upcoming version, being equal is the normal case; the guard against releasing the
    # same X.Y.Z twice is the published-version check below. That way a --no-build run (which sets
    # X.Y.Z+Build without publishing) does not block the later release either.
    if new < parsed[0]:
        return "--version %s is lower than the version in the files (%s)." % (version, dotted(parsed[0]))
    published = last_published(entries, release_dir)
    if published is not None and new <= published:
        return "The updater compares only X.Y.Z: %s must be higher than the published %s." % (
            version,
            dotted(published),
        )
    return None


def next_problem(version, file_version, entries, release_dir):
    """Why --next X.Y.Z is not allowed, or None. The highest version so far must be released
    (dated), and the new one must be higher than everything before it."""
    if not BASE_VERSION.match(version):
        return "--next expects X.Y.Z, e.g. 0.3.0"
    new = tuple(int(part) for part in version.split("."))
    versions = changelog_versions(entries)
    if versions:
        highest, date = max(versions, key=lambda pair: pair[0])
        if date is None:
            return "%s is not released yet (changelog entry without date); release this version first." % dotted(
                highest
            )
        if new <= highest:
            return "--next %s must be higher than %s in the changelog." % (version, dotted(highest))
    parsed = parse_version(file_version)
    if parsed is not None and new <= parsed[0]:
        return "--next %s must be higher than the version in the files (%s)." % (version, file_version)
    published = last_published(entries, release_dir)
    if published is not None and new <= published:
        return "--next %s must be higher than the published %s." % (version, dotted(published))
    return None


def with_next_entry(entries, version):
    """Changelog with an empty, undated entry for the upcoming version at the top."""
    return [{"version": version, "date": None, "items": []}, *entries]


def suggest_next(version):
    """Suggestion after a release: the next minor version, 0.2.0 -> 0.3.0."""
    major, minor, _ = (int(part) for part in version.split("."))
    return "%d.%d.0" % (major, minor + 1)


def prepare_next(version):
    """--next: version files to the upcoming version (without build number), plus the changelog entry."""
    entries, _ = find_changelog_entry(version)
    problem = next_problem(version, current_version(), entries, RELEASE_DIR)
    if problem:
        fail(problem)
    set_version(version)
    write_json(CHANGELOG_JSON, with_next_entry(entries, version))
    print("Upcoming version %s set and added empty to %s." % (version, CHANGELOG_JSON.relative_to(ROOT).as_posix()))
    print("Add items there as the work lands; commit the files now.")


def display(internal):
    """0.12.224+1548 -> 0.12.224-1548 (for display and file names; '+' is tricky in URLs)."""
    return internal.replace("+", "-")


def github_tag(base_version):
    """0.2.0 -> v0.2.0. The tag names the release, not the build, so it stays stable."""
    return "v%s" % base_version


def github_base_url(base_version):
    return "https://github.com/%s/releases/download/%s" % (GITHUB_REPO, github_tag(base_version))


def sha256_hex(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checksums_text(target, names):
    """Lines in the format of `sha256sum` (two spaces, text mode), so `sha256sum -c` accepts them."""
    return "".join("%s  %s\n" % (sha256_hex(target / name), name) for name in names)


def write_checksums(target, names):
    # Bytes instead of text mode: on Windows, text mode would write CRLF, which sha256sum -c rejects.
    (target / CHECKSUMS_NAME).write_bytes(checksums_text(target, names).encode("utf-8"))


def release_files(target, setup_name):
    """Files to attach or upload, in order. latest.json stays last: it announces the update."""
    return [target / setup_name, target / "Hangar-Setup.exe", target / CHECKSUMS_NAME, target / "latest.json"]


def github_release_commands(base_version, version, target, setup_name, notes_file):
    """The gh commands that publish a built release, as argv lists (never a shell string).

    Draft first and publish last: GitHub's /releases/latest skips drafts, so installed apps see the
    update only once every file is attached.
    """
    tag = github_tag(base_version)
    files = [str(path) for path in release_files(target, setup_name)]
    return [
        ["git", "push", "origin", "main"],
        [
            "gh", "release", "create", tag, *files,
            "--repo", GITHUB_REPO,
            "--target", "main",
            "--title", "Hangar %s" % display(version),
            "--notes-file", str(notes_file),
            "--draft",
        ],
        ["gh", "release", "edit", tag, "--repo", GITHUB_REPO, "--draft=false", "--latest"],
    ]


def quoted(argv):
    """Readable PowerShell form of an argv list for copying; only for printing."""
    return " ".join('"%s"' % arg if (" " in arg or not arg) else arg for arg in argv)


# Prefix of the variables through which Tauri takes the updater key, its path and its password.
UPDATER_KEY_VARIABLES = "TAURI_SIGNING_PRIVATE_KEY"


def without_updater_key(environ):
    """A copy of `environ` without any updater key variable (Windows names ignore case)."""
    return {
        name: value
        for name, value in environ.items()
        if not name.upper().startswith(UPDATER_KEY_VARIABLES)
    }


def build_config(signing):
    """The --config override for `tauri build`: updater artifacts off, Authenticode if set.

    `tauri build` would sign the installer itself and needs the key in its environment for that,
    which every Node and Cargo build process would inherit; sign_update() signs it afterwards.
    """
    config = {"bundle": {"createUpdaterArtifacts": False}}
    if signing is not None:
        config["bundle"].update(signing.get("bundle", {}))
    return config


def locked_cli_version(lock_text):
    """The @tauri-apps/cli version the root importer of pnpm-lock.yaml pins, or None."""
    match = re.search(
        r"(?m)^      '@tauri-apps/cli':\n        specifier: [^\n]*\n        version: ([^\s(]+)", lock_text
    )
    return match.group(1) if match else None


def cli_version_problem(package_text, lock_text):
    """None when the installed CLI has the version the lock file pins, otherwise the reason."""
    locked = locked_cli_version(lock_text)
    if locked is None:
        return "pnpm-lock.yaml names no version of @tauri-apps/cli"
    try:
        installed = json.loads(package_text).get("version")
    except (ValueError, AttributeError):
        installed = None
    if installed != locked:
        return "node_modules has @tauri-apps/cli %s, but pnpm-lock.yaml pins %s; run pnpm install --frozen-lockfile" % (
            installed,
            locked,
        )
    return None


def cli_problem():
    """Compares the installed Tauri CLI with the lock file. Only a consistency check: whoever can
    write node_modules can change both, see release-rollout.md (residual risk)."""
    try:
        package_text = TAURI_CLI_PACKAGE.read_text(encoding="utf-8")
        lock_text = PNPM_LOCK.read_text(encoding="utf-8")
    except OSError as error:
        return "Tauri CLI or pnpm-lock.yaml not readable: %s (run pnpm install)" % error
    return cli_version_problem(package_text, lock_text)


def tauri_command(*args):
    """argv for the Tauri CLI: node.exe with the CLI's tauri.js, then `args`.

    pnpm and npx are .cmd files on Windows; CreateProcess runs those through cmd.exe, which
    interprets `&`, `%` and quotes in every argument (BatBadBut). node.exe takes the argv as is.
    """
    node = resolve_tool("node")
    if not TAURI_JS.is_file():
        fail("Tauri CLI missing: %s (run pnpm install)" % TAURI_JS)
    problem = cli_problem()
    if problem:
        fail(problem)
    return [node, str(TAURI_JS), *args]


# The only variables the signer inherits: what Windows, macOS and Linux programs need to start and
# find their temporary and profile folders. Everything else stays out, above all NODE_OPTIONS and
# NODE_PATH, through which node would load other code into the process that holds the key.
SIGNER_VARIABLES = frozenset(
    name.upper()
    for name in (
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "SystemDrive",
        "windir",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "LANG",
        "LC_ALL",
    )
)


def signer_environment(environ):
    """The reduced environment of the signer process, without any updater key variable."""
    env = {name: value for name, value in environ.items() if name.upper() in SIGNER_VARIABLES}
    env["NoDefaultCurrentDirectoryInExePath"] = "1"
    return env


def build(signing):
    env = without_updater_key(os.environ)
    command = tauri_command("build")
    config_file = None
    try:
        # A file instead of a JSON argument keeps the value out of the process list.
        # `tauri build --config` accepts JSON strings or file paths.
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", suffix=".json", prefix="hangar-sign-", delete=False
        ) as handle:
            json.dump(build_config(signing), handle)
            config_file = Path(handle.name)
        command += ["--config", str(config_file)]
        result = subprocess.run(command, cwd=ROOT, env=env, check=False)
    finally:
        if config_file is not None:
            config_file.unlink(missing_ok=True)
    if result.returncode != 0:
        fail("tauri build failed (exit code %d)" % result.returncode)


def minisign_key_id(encoded):
    """The 8-byte key id of a Tauri updater signature or public key, as hex.

    Both are base64 of minisign's text form: a comment line, then base64 of two algorithm bytes,
    the key id and the key or signature. minisign shows the id as a little-endian number (the
    "minisign public key: ..." comment), so the bytes are printed in reverse to match it. None
    when the text has another shape.
    """
    try:
        text = base64.b64decode(encoded.strip(), validate=True).decode("utf-8")
        lines = text.splitlines()
        blob = base64.b64decode(lines[1].strip(), validate=True)
    except (ValueError, IndexError, UnicodeDecodeError):
        return None
    if len(blob) < 10:
        return None
    return blob[2:10][::-1].hex().upper()


def updater_pubkey():
    """The public key the app checks updates against (tauri.conf.json, plugins.updater.pubkey)."""
    return read_json(TAURI_CONF)["plugins"]["updater"]["pubkey"]


def signature_key_problem(signature_text, pubkey):
    """None when the signature was made with the key whose public half the app ships."""
    signed = minisign_key_id(signature_text)
    if signed is None:
        return "the updater signature cannot be read"
    shipped = minisign_key_id(pubkey)
    if shipped is None:
        return "the updater public key in tauri.conf.json cannot be read"
    if signed != shipped:
        return (
            "the installer was signed with key %s, but the app trusts only key %s; "
            "installed copies would refuse this update" % (signed, shipped)
        )
    return None


KEY_UNLOCK_VARIABLE = "TAURI_SIGNING_PRIVATE_KEY_PASSWORD"


def unlock_variable_problem(environ):
    """Why the key password must not come from `environ`, or None.

    A password in the environment of the release shell reached every program started from that
    shell before (installs, editors, agents), so it is refused even when the key has none; the
    script asks for it instead. Names only the variable, never its value.
    """
    if any(name.upper() == KEY_UNLOCK_VARIABLE for name in environ):
        return (
            "%s is set in the environment, where every program started from this shell can read it. "
            "Unset it (PowerShell: Remove-Item Env:%s); the script asks for the password when it signs."
            % (KEY_UNLOCK_VARIABLE, KEY_UNLOCK_VARIABLE)
        )
    return None


def key_password(allow_unencrypted_key):
    """The updater key's password, typed at the console; never taken from the environment.

    An empty password is refused unless --allow-unencrypted-key says the key has none.
    """
    if allow_unencrypted_key:
        print(
            "Warning: the updater key is used with no password (--allow-unencrypted-key). Anyone who "
            "can read the key file can sign updates; give it a password (release-rollout.md)."
        )
        return ""
    try:
        password = getpass.getpass("Password of the updater key: ")
    except EOFError:
        password = ""
    if not password:
        fail(
            "no password for the updater key. Give the key one (release-rollout.md), or pass "
            "--allow-unencrypted-key if it really has none."
        )
    return password


# What sign_update() hands to collect(): the `.sig`, its text as read once and the installer's
# SHA-256 that held from the Authenticode check through the updater signer.
Signed = collections.namedtuple("Signed", "signature text sha256")
# The signature verifier built before `tauri build`, and its SHA-256 at that point.
Verifier = collections.namedtuple("Verifier", "path sha256")


def sign_update(key_path, setup, version, expected_sha256, allow_unencrypted_key=False):
    """Signs the finished installer for the updater and returns what collect() needs (Signed).

    Runs after Authenticode signing, which changes the file. `expected_sha256` is the installer's
    hash taken right after the Authenticode check: the signer signs those bytes or nothing, and
    they must be the same after it. `--app-version` binds the version into the signed trusted
    comment, as `tauri build` does (`version:<version>`); the updater compares it with the
    version latest.json announces. The password goes only into the signer's environment, never
    into os.environ, so no other child process sees it.
    """
    problem = unlock_variable_problem(os.environ)
    if problem:
        fail(problem)
    if sha256_hex(setup) != expected_sha256:
        fail("%s changed after its Authenticode signature was checked; nothing was signed" % setup.name)
    env = signer_environment(os.environ)
    # Tauri accepts a path or the content; the content avoids trouble with spaces in the path.
    env["TAURI_SIGNING_PRIVATE_KEY"] = key_path.read_text(encoding="utf-8").strip()
    # Always set: without the variable the signer assumes an empty password (Tauri CLI 2.12.4).
    env[KEY_UNLOCK_VARIABLE] = key_password(allow_unencrypted_key)
    signature = setup.with_name(setup.name + ".sig")
    # Only a signature written by this run counts.
    signature.unlink(missing_ok=True)
    command = tauri_command("signer", "sign", "--app-version", version, str(setup))
    result = subprocess.run(command, cwd=ROOT, env=env, check=False, stdin=subprocess.DEVNULL)
    del env
    if result.returncode != 0:
        fail("tauri signer sign failed (exit code %d)" % result.returncode)
    if not signature.is_file():
        fail("updater signature missing: %s" % signature)
    if sha256_hex(setup) != expected_sha256:
        signature.unlink(missing_ok=True)
        fail("%s changed while it was signed for the updater" % setup.name)
    text = signature.read_text(encoding="utf-8").strip()
    # A wrong key file would sign without complaint, and every installed copy would then refuse
    # the update; caught here before anything is collected. collect() proves the signature.
    problem = signature_key_problem(text, updater_pubkey())
    if problem:
        signature.unlink(missing_ok=True)
        fail(problem)
    return Signed(signature, text, expected_sha256)


VERIFIER_NAME = "verify_update_sig"


def build_verifier():
    """Builds src-tauri/examples/verify_update_sig.rs before `tauri build` and returns it.

    `cargo run` after signing would compile, and build scripts would run, between signing and
    collecting the installer. The path comes from Cargo's own report, so CARGO_TARGET_DIR or a
    profile setting cannot send the check to another file.
    """
    command = [
        resolve_tool("cargo"),
        "build",
        "--locked",
        "--offline",
        "--message-format=json",
        "--manifest-path",
        str(CARGO_TOML),
        "--example",
        VERIFIER_NAME,
    ]
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=without_updater_key(os.environ),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if result.returncode != 0:
        fail("building the signature verifier failed (exit code %d)" % result.returncode)
    executable = None
    for line in (result.stdout or "").splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if not isinstance(message, dict):
            continue
        target = message.get("target")
        if (
            message.get("reason") == "compiler-artifact"
            and isinstance(target, dict)
            and target.get("name") == VERIFIER_NAME
            and "example" in (target.get("kind") or [])
            and isinstance(message.get("executable"), str)
        ):
            executable = Path(message["executable"])
    if executable is None or not executable.is_file():
        fail("cargo built no %s example" % VERIFIER_NAME)
    return Verifier(executable, sha256_hex(executable))


def update_signature_problem(verifier, setup, signature, version):
    """Checks the signature the way installed apps do: minisign's signature over the installer and
    the global signature over the trusted comment, which must carry `version:<version>`
    (src-tauri/examples/verify_update_sig.rs, the same crate and call as tauri-plugin-updater).
    The key id check above only compares labels; this proves the signature. None when it holds."""
    if not verifier.path.is_file() or sha256_hex(verifier.path) != verifier.sha256:
        return "the signature verifier changed after it was built; nothing was verified"
    command = [str(verifier.path), str(setup), str(signature), updater_pubkey(), version]
    result = subprocess.run(
        command, cwd=ROOT, env=without_updater_key(os.environ), check=False, stdin=subprocess.DEVNULL
    )
    if result.returncode != 0:
        return "the updater signature failed the verification installed apps run (reason above)"
    return None


def find_setup(built_after):
    # Tauri names the file after the internal version; take only the fresh output of this run.
    candidates = [
        path
        for path in BUNDLE_DIR.glob("Hangar_*_x64-setup.exe")
        if path.stat().st_mtime >= built_after
    ]
    if len(candidates) != 1:
        fail("expected exactly one new installer in %s, found: %d" % (BUNDLE_DIR, len(candidates)))
    return candidates[0]


# The signature text the verifier checks in collect(), next to the copies; removed right after.
VERIFY_SIGNATURE_NAME = ".updater-signature.tmp"


def collect(setup, signed, version, base_url, notes, verifier):
    """Copies the signed installer under release/ and writes the checksums and latest.json.

    The copies must have the SHA-256 that held through signing; a file changed in between (by
    anything that ran after the signer) aborts and leaves nothing behind. The verifier then
    checks the copy that is uploaded against `signed.text`, the signature latest.json announces,
    written to a file of its own: neither a changed installer nor a changed `.sig` can slip
    between what was verified and what is published.
    """
    shown = display(version)
    setup_name = "Hangar_%s_x64-setup.exe" % shown
    target = RELEASE_DIR / shown
    target.mkdir(parents=True, exist_ok=True)
    copies = [target / setup_name, target / "Hangar-Setup.exe"]

    def discard(message):
        for copy in copies:
            copy.unlink(missing_ok=True)
        fail(message)

    for copy in copies:
        shutil.copy2(setup, copy)
    if any(sha256_hex(copy) != signed.sha256 for copy in copies):
        discard("%s changed after it was signed; nothing was collected" % setup.name)
    text_file = target / VERIFY_SIGNATURE_NAME
    try:
        text_file.write_text(signed.text, encoding="utf-8")
        problem = update_signature_problem(verifier, copies[0], text_file, version)
    finally:
        text_file.unlink(missing_ok=True)
    if problem:
        signed.signature.unlink(missing_ok=True)
        discard(problem)
    write_checksums(target, [setup_name, "Hangar-Setup.exe"])

    manifest = {
        "version": version,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat(),
        "platforms": {
            "windows-x86_64": {
                "signature": signed.text,
                "url": "%s/%s" % (base_url.rstrip("/"), setup_name),
            }
        },
    }
    write_json(target / "latest.json", manifest)
    return target, setup_name


def main():
    parser = argparse.ArgumentParser(description="Build a Hangar release (installer + latest.json)")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--version",
        help="version X.Y.Z to release (the highest in the changelog); the build number comes from Git",
    )
    mode.add_argument(
        "--next",
        metavar="X.Y.Z",
        help="after a release: put the upcoming version into the files and add it empty to the changelog",
    )
    parser.add_argument("--no-build", action="store_true", help="only set the version, do not build")
    parser.add_argument("--allow-dirty", action="store_true", help="build even with uncommitted changes")
    parser.add_argument(
        "--allow-unencrypted-key",
        action="store_true",
        help="sign with an updater key that has no password (otherwise the script asks for it)",
    )
    parser.add_argument(
        "--notes",
        default=None,
        help="text the app shows for the update (default: the version's changelog entry)",
    )
    parser.add_argument(
        "--base-url",
        default=None,
        help="self-hosted download folder instead of a GitHub release (default: GitHub)",
    )
    args = parser.parse_args()
    # Windows would otherwise search the working folder for programs before PATH, also in every
    # child process (cargo, node, the signer).
    os.environ["NoDefaultCurrentDirectoryInExePath"] = "1"

    if args.next is not None:
        # Needs neither the key nor a build; it only changes files in the repo.
        prepare_next(args.next)
        return

    key_path = Path(os.environ.get("HANGAR_SIGNING_KEY", DEFAULT_KEY))
    if not key_path.is_file():
        fail("updater signing key missing: %s (without it there are no updates)" % key_path)
    if args.base_url is not None and not args.base_url.startswith("https://"):
        fail("the base URL must start with https://; the updater requires TLS.")
    # Check before changing anything, so a wrong variable never leaves a half-done change.
    try:
        signing = windows_signing_config(os.environ)
    except SigningError as error:
        fail(str(error))

    entries, entry = find_changelog_entry(args.version)
    problem = release_problem(args.version, current_version(), entries, RELEASE_DIR)
    if problem:
        fail(problem)
    if entry is None:
        fail("no changelog entry for %s in %s" % (args.version, CHANGELOG_JSON.relative_to(ROOT)))
    if not args.no_build:
        # Before the build, which takes minutes, not only when signing.
        problem = unlock_variable_problem(os.environ)
        if problem:
            fail(problem)
        if not args.allow_dirty and working_tree_dirty():
            fail("the working tree is not clean. Commit first (or pass --allow-dirty).")
        # Not skipped by --allow-dirty: these files never show up as changes.
        check_build_inputs()

    # Check before setting the version, so a missing entry never leaves a half-done change.
    # The date belongs to the release, not to where the notes come from, so set it with --notes too.
    # The entry exists (release_problem); it needs items even with --notes, otherwise "What's New"
    # shows nothing for the released version.
    notes = args.notes
    if not args.no_build:
        entry_notes = changelog_notes(entry)
        if notes is None:
            notes = entry_notes
        date_changelog(entries, entry)
        print("Notes for latest.json:\n%s" % notes)

    version = "%s+%d" % (args.version, build_number())
    set_version(version)
    print("Version %s (shown as %s) set. Commit after the release." % (version, display(version)))
    if args.no_build:
        return

    if signing is None:
        print(
            "Note: unsigned (neither HANGAR_CERT_THUMBPRINT nor HANGAR_SIGN_COMMAND is set); "
            "Windows SmartScreen warns on the installer's first start."
        )
    else:
        print("Authenticode code signing active (setting from the environment, values are not printed).")

    verifier = build_verifier()
    started = time.time()
    build(signing)
    setup = find_setup(started)
    if signing is not None:
        # According to the Tauri repo it signs the program, the uninstaller and the installer; the
        # docs do not say so explicitly for NSIS, so verify here instead of trusting it.
        # Taken around the check: the updater signer must sign exactly the bytes it accepted.
        verified_sha256 = sha256_hex(setup)
        verify_signature(setup)
        if sha256_hex(setup) != verified_sha256:
            fail("%s changed while its Authenticode signature was checked" % setup.name)
        if MAIN_EXE.is_file() and MAIN_EXE.stat().st_mtime >= started:
            verify_signature(MAIN_EXE)
    else:
        verified_sha256 = sha256_hex(setup)
    # After Authenticode, which rewrites the installer: the signature covers the final bytes.
    signed = sign_update(key_path, setup, version, verified_sha256, args.allow_unencrypted_key)
    base_url = args.base_url or github_base_url(args.version)
    target, setup_name = collect(setup, signed, version, base_url, notes or "", verifier)
    print()
    print("Done: %s" % target)
    if args.base_url is None:
        notes_file = target / "release-notes.md"
        notes_file.write_text((notes or "") + "\n", encoding="utf-8")
        print('Commit the release state first ("chore: Version %s"), then publish on GitHub:' % display(version))
        for argv in github_release_commands(args.version, version, target, setup_name, notes_file):
            print("  %s" % quoted(argv))
        print("Older installations only poll %s; copy latest.json there as well." % LEGACY_UPDATE_URL)
    else:
        print("Upload to %s/ in this order:" % args.base_url.rstrip("/"))
        print("  1. %s" % setup_name)
        print("  2. Hangar-Setup.exe   (download link of the website)")
        print("  3. %s     (checksums of both installers)" % CHECKSUMS_NAME)
        print("  4. latest.json        (last: announces the update)")
    print()
    # Rule "the code always carries the upcoming version" (ADR 0002): otherwise the next dev build
    # shows the old number again while "What's New" is already collecting new items.
    print("Then: commit the release state and create the upcoming version with an empty changelog entry:")
    print("  py -3 tools/release.py --next %s" % suggest_next(args.version))


if __name__ == "__main__":
    main()
