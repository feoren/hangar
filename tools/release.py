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
2. Builds with `pnpm tauri build` and signs the update file with the private key
   (default: %USERPROFILE%\.tauri\hangar.key, override with HANGAR_SIGNING_KEY).
3. Puts everything to upload under release/<version>/:
     Hangar_<x.y.z-build>_x64-setup.exe   installer for this version (target of the update URL)
     Hangar-Setup.exe                 same file under a fixed name for the download link
     latest.json                      polled by installed apps
4. Publish. By default the files go to a GitHub release with the tag vX.Y.Z; the script prints the
   `gh` commands for it (it never publishes by itself). The release is created as a draft, the files
   are attached, and only then is it published, so .../releases/latest/download/latest.json never
   announces a version whose installer is missing. With --base-url, upload to that folder instead:
   installers first and latest.json last.

Authenticode code signing of the installer (against the SmartScreen warning), optional via environment:
  HANGAR_CERT_THUMBPRINT   SHA-1 thumbprint of a certificate in the Windows certificate store;
                           Tauri then signs with signtool (SHA-256, timestamped).
  HANGAR_TIMESTAMP_URL     timestamp server for it (default http://time.certum.pl).
  HANGAR_TIMESTAMP_TSP     1 (default) = RFC 3161 (signtool /tr), 0 = legacy Authenticode (/t).
  HANGAR_SIGN_COMMAND      alternatively, a custom signing command as a JSON argv with "%1" for the file,
                           e.g. ["signtool.exe","sign","/sha1","<thumbprint>","/fd","sha256",
                           "/tr","http://time.certum.pl","/td","sha256","%1"].
  Without these variables the script builds unsigned as before and says so. With them, the setting
  goes to `pnpm tauri build` as a --config file (tauri.conf.json stays unchanged), and after the
  build Get-AuthenticodeSignature checks the installer and the program: without a valid, timestamped
  signature the script aborts. Process and vendors: docs/wiki/pages/syntheses/release-rollout.md.

No dependencies beyond Python 3 and the project toolchain. Never prints keys, thumbprints or
signing commands.
"""
import argparse
import datetime
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
GITHUB_REPO = "feoren/hangar"
# Older installations only poll this server; it gets a copy of latest.json (see release-rollout.md).
LEGACY_UPDATE_URL = "https://hangar.feoren.com/download/"
DEFAULT_KEY = Path.home() / ".tauri" / "hangar.key"
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
    shell = shutil.which("pwsh") or shutil.which("powershell")
    if not shell:
        fail("pwsh/powershell not found; cannot check the signature of %s" % path.name)
    env = dict(os.environ)
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


def git(*args):
    result = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        fail("git %s failed: %s" % (" ".join(args), result.stderr.strip()))
    return result.stdout.strip()


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


def github_release_commands(base_version, version, target, setup_name, notes_file):
    """The gh commands that publish a built release, as argv lists (never a shell string).

    Draft first and publish last: GitHub's /releases/latest skips drafts, so installed apps see the
    update only once every file is attached.
    """
    tag = github_tag(base_version)
    files = [str(target / setup_name), str(target / "Hangar-Setup.exe"), str(target / "latest.json")]
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


def build(key_path, signing):
    pnpm = shutil.which("pnpm")
    if not pnpm:
        fail("pnpm not found")
    env = dict(os.environ)
    # Tauri accepts a path or the content; the content avoids trouble with spaces in the path.
    env["TAURI_SIGNING_PRIVATE_KEY"] = key_path.read_text(encoding="utf-8").strip()
    env.setdefault("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "")
    command = [pnpm, "tauri", "build"]
    config_file = None
    try:
        if signing is not None:
            # A file instead of a JSON argument: on Windows pnpm is a .cmd file, and cmd.exe mangles
            # quotes in arguments. It also keeps the value out of the process list.
            # `tauri build --config` accepts JSON strings or file paths.
            with tempfile.NamedTemporaryFile(
                "w", encoding="utf-8", suffix=".json", prefix="hangar-sign-", delete=False
            ) as handle:
                json.dump(signing, handle)
                config_file = Path(handle.name)
            command += ["--config", str(config_file)]
        result = subprocess.run(command, cwd=ROOT, env=env, check=False)
    finally:
        if config_file is not None:
            config_file.unlink(missing_ok=True)
    if result.returncode != 0:
        fail("pnpm tauri build failed (exit code %d)" % result.returncode)


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


def collect(setup, version, base_url, notes):
    signature = setup.with_name(setup.name + ".sig")
    if not signature.is_file():
        fail("updater signature missing: %s" % signature)

    shown = display(version)
    setup_name = "Hangar_%s_x64-setup.exe" % shown
    target = RELEASE_DIR / shown
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(setup, target / setup_name)
    shutil.copy2(setup, target / "Hangar-Setup.exe")

    manifest = {
        "version": version,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat(),
        "platforms": {
            "windows-x86_64": {
                "signature": signature.read_text(encoding="utf-8").strip(),
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
    if not args.no_build and not args.allow_dirty and git("status", "--porcelain"):
        fail("the working tree is not clean. Commit first (or pass --allow-dirty).")

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

    started = time.time()
    build(key_path, signing)
    setup = find_setup(started)
    if signing is not None:
        # According to the Tauri repo it signs the program, the uninstaller and the installer; the
        # docs do not say so explicitly for NSIS, so verify here instead of trusting it.
        verify_signature(setup)
        if MAIN_EXE.is_file() and MAIN_EXE.stat().st_mtime >= started:
            verify_signature(MAIN_EXE)
    base_url = args.base_url or github_base_url(args.version)
    target, setup_name = collect(setup, version, base_url, notes or "")
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
        print("  3. latest.json        (last: announces the update)")
    print()
    # Rule "the code always carries the upcoming version" (ADR 0002): otherwise the next dev build
    # shows the old number again while "What's New" is already collecting new items.
    print("Then: commit the release state and create the upcoming version with an empty changelog entry:")
    print("  py -3 tools/release.py --next %s" % suggest_next(args.version))


if __name__ == "__main__":
    main()
