"""Independent reference for the test vectors in src/hash.rs and src/e2e.rs and the character
table in src/text.rs (Python standard library only).

Prints the vectors and exits with an error when one differs from its constant in src/hash.rs or
src/e2e.rs, or when the format character ranges in src/text.rs differ from Python's unicodedata.

Run: py -3 crates/hangar-team-protocol/test_vectors.py
"""

import base64
import hashlib
import hmac
import pathlib
import re
import struct
import sys
import unicodedata

SRC = pathlib.Path(__file__).parent / "src"

# Name printed here -> constant in src/hash.rs.
CONSTANTS = {
    "TASK": "VECTOR_TASK",
    "REPO": "VECTOR_REPO",
    "AREA": "VECTOR_AREA",
    "FILE": "VECTOR_FILE",
    "FILE_SENSITIVE": "VECTOR_FILE_SENSITIVE",
    "FILE_NFC": "VECTOR_FILE_NFC",
    "DERIVED_TASK": "VECTOR_DERIVED_TASK",
    "DERIVED_SEAL_KEY": "VECTOR_DERIVED_SEAL_KEY",
    "LOG_1": "VECTOR_LOG_1",
    "LOG_2": "VECTOR_LOG_2",
}

# Membership log entries of the vectors: name in src/e2e.rs -> raw bytes.
LOG_ENTRIES = {
    "LOG_ENTRY_1": b"hangar member log entry 1",
    "LOG_ENTRY_2": bytes(range(40)),
}

# The Unicode version src/text.rs was generated from.
UNICODE_VERSION = "16.0.0"


def fields(*parts: bytes) -> bytes:
    return b"".join(struct.pack(">I", len(part)) + part for part in parts)


def keyed(key: bytes, label: bytes, *parts: bytes) -> bytes:
    return hmac.new(key, fields(label, *parts), hashlib.sha256).digest()


def hkdf_sha256(ikm: bytes, info: bytes) -> bytes:
    """RFC 5869 with no salt (32 zero bytes), one block of output."""
    prk = hmac.new(b"\x00" * 32, ikm, hashlib.sha256).digest()
    return hmac.new(prk, info + b"\x01", hashlib.sha256).digest()


def nfc(value: str) -> bytes:
    return unicodedata.normalize("NFC", value).encode("utf-8")


def memberlog_hash(index: int, prev: bytes | None, entry: bytes) -> bytes:
    """SHA-256 over the label, the index (8 bytes big endian), prev (empty for the first entry)
    and the entry's raw bytes, each length-prefixed."""
    fields_ = fields(b"hangar-team/v1/memberlog", struct.pack(">Q", index), prev or b"", entry)
    return hashlib.sha256(fields_).digest()


def base64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def rust_sources() -> str:
    return "".join((SRC / name).read_text(encoding="utf-8") for name in ("hash.rs", "e2e.rs"))


def rust_constants() -> dict[str, str]:
    return dict(re.findall(r'const (VECTOR_\w+): &str =\s*"([0-9a-f]{64})"', rust_sources()))


def rust_log_entries() -> dict[str, str]:
    return dict(re.findall(r'const (LOG_ENTRY_\d+): &str =\s*"([A-Za-z0-9_-]+)"', rust_sources()))


def rust_format_ranges() -> list[tuple[int, int]]:
    source = (SRC / "text.rs").read_text(encoding="utf-8")
    return [
        (int(low, 16), int(high, 16))
        for low, high in re.findall(r"\(0x([0-9A-F]+), 0x([0-9A-F]+)\)", source)
    ]


def python_format_ranges() -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for code in range(0x110000):
        if unicodedata.category(chr(code)) != "Cf":
            continue
        if ranges and ranges[-1][1] == code - 1:
            ranges[-1] = (ranges[-1][0], code)
        else:
            ranges.append((code, code))
    return ranges


def main() -> int:
    key = bytes(range(32))
    repo = b"4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    vectors = {
        "TASK": keyed(key, b"hangar-team/v1/task", b"gh:owner/repo#12"),
        "REPO": keyed(key, b"hangar-team/v1/repo", repo),
        "AREA": keyed(key, b"hangar-team/v1/area", repo, b"fold", b"src"),
        "FILE": keyed(key, b"hangar-team/v1/file", repo, b"fold", b"src/auth/login.rs"),
        "FILE_SENSITIVE": keyed(
            key, b"hangar-team/v1/file", repo, b"sensitive", b"src/auth/login.rs"
        ),
        "FILE_NFC": keyed(
            key, b"hangar-team/v1/file", repo, b"fold", nfc("Café/Menu.md".lower())
        ),
    }
    team_info = (b"team_1", struct.pack(">I", 3))
    secret = bytes([7] * 32)
    match_key = hkdf_sha256(secret, fields(b"hangar-team/v1/match-key", *team_info))
    vectors["DERIVED_TASK"] = keyed(match_key, b"hangar-team/v1/task", b"t-1")
    vectors["DERIVED_SEAL_KEY"] = hkdf_sha256(secret, fields(b"hangar-team/v1/seal-key", *team_info))
    log_1 = memberlog_hash(1, None, LOG_ENTRIES["LOG_ENTRY_1"])
    vectors["LOG_1"] = log_1
    vectors["LOG_2"] = memberlog_hash(2, log_1, LOG_ENTRIES["LOG_ENTRY_2"])

    expected = rust_constants()
    failed = False
    for name, value in vectors.items():
        print(f"{name} {value.hex()}")
        if expected.get(CONSTANTS[name]) != value.hex():
            print(f"  differs from {CONSTANTS[name]} in src/hash.rs or src/e2e.rs", file=sys.stderr)
            failed = True

    entries = rust_log_entries()
    for name, raw in LOG_ENTRIES.items():
        if entries.get(name) != base64url(raw):
            print(f"{name} in src/e2e.rs is not the base64url of its bytes", file=sys.stderr)
            failed = True

    if unicodedata.unidata_version != UNICODE_VERSION:
        print(
            f"Python has Unicode {unicodedata.unidata_version}, src/text.rs is {UNICODE_VERSION}; "
            "table check skipped",
            file=sys.stderr,
        )
    elif rust_format_ranges() != python_format_ranges():
        print("FORMAT_RANGES in src/text.rs differ from unicodedata (Cf)", file=sys.stderr)
        failed = True
    else:
        print(f"FORMAT_RANGES match Unicode {UNICODE_VERSION}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
