"""Independent reference for the test vectors in src/tests.rs (Python standard library only).

Implements Ed25519 (RFC 8032), X25519 (RFC 7748), ChaCha20-Poly1305 and XChaCha20-Poly1305
(RFC 8439, draft-irtf-cfrg-xchacha-03), HKDF (RFC 5869) and HPKE base mode with
DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305 (RFC 9180) from their specifications,
checks each against a published test vector, then recomputes every VECTOR_* constant of
src/tests.rs and exits with an error when one differs.

Slow on purpose (textbook big-integer arithmetic); the inputs are fixed, nothing here is secret.

Run: py -3 crates/hangar-team-crypto/test_vectors.py
"""

import base64
import hashlib
import hmac
import pathlib
import re
import struct
import sys

SRC = pathlib.Path(__file__).parent / "src" / "tests.rs"

# ---------------------------------------------------------------------------------------------
# Ed25519, RFC 8032 section 6 (reference implementation, rewritten)
# ---------------------------------------------------------------------------------------------

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)


def _recover_x(y, sign):
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P)
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P != 0:
        return None
    if (x & 1) != sign:
        x = P - x
    return x


G_Y = 4 * pow(5, P - 2, P) % P
G = (_recover_x(G_Y, 0), G_Y, 1, _recover_x(G_Y, 0) * G_Y % P)


def _add(a, b):
    A = (a[1] - a[0]) * (b[1] - b[0]) % P
    B = (a[1] + a[0]) * (b[1] + b[0]) % P
    C = 2 * a[3] * b[3] * D % P
    Dd = 2 * a[2] * b[2] % P
    E, F, Gg, H = B - A, Dd - C, Dd + C, B + A
    return (E * F, Gg * H, F * Gg, E * H)


def _mul(s, point):
    q = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            q = _add(q, point)
        point = _add(point, point)
        s >>= 1
    return q


def _compress(point):
    zinv = pow(point[2], P - 2, P)
    x = point[0] * zinv % P
    y = point[1] * zinv % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _sha512_int(data):
    return int.from_bytes(hashlib.sha512(data).digest(), "little")


def _expand_seed(seed):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def ed25519_public(seed):
    a, _ = _expand_seed(seed)
    return _compress(_mul(a, G))


def ed25519_sign(seed, message):
    a, prefix = _expand_seed(seed)
    public = _compress(_mul(a, G))
    r = _sha512_int(prefix + message) % L
    big_r = _compress(_mul(r, G))
    h = _sha512_int(big_r + public + message) % L
    s = (r + h * a) % L
    return big_r + int.to_bytes(s, 32, "little")


# ---------------------------------------------------------------------------------------------
# X25519, RFC 7748 section 5
# ---------------------------------------------------------------------------------------------


def _clamp(k):
    k = bytearray(k)
    k[0] &= 248
    k[31] &= 127
    k[31] |= 64
    return bytes(k)


def x25519(k, u):
    k = int.from_bytes(_clamp(k), "little")
    u = int.from_bytes(u, "little") & ((1 << 255) - 1)
    x1, x2, z2, x3, z3, swap = u, 1, 0, u, 1, 0
    for t in reversed(range(255)):
        bit = (k >> t) & 1
        swap ^= bit
        if swap:
            x2, x3, z2, z3 = x3, x2, z3, z2
        swap = bit
        a, b = (x2 + z2) % P, (x2 - z2) % P
        aa, bb = a * a % P, b * b % P
        e = (aa - bb) % P
        c, d = (x3 + z3) % P, (x3 - z3) % P
        da, cb = d * a % P, c * b % P
        x3 = (da + cb) ** 2 % P
        z3 = x1 * (da - cb) ** 2 % P
        x2 = aa * bb % P
        z2 = e * (aa + 121665 * e) % P
    if swap:
        x2, z2 = x3, z3
    return int.to_bytes(x2 * pow(z2, P - 2, P) % P, 32, "little")


def x25519_public(k):
    return x25519(k, int.to_bytes(9, 32, "little"))


# ---------------------------------------------------------------------------------------------
# ChaCha20, Poly1305, AEADs (RFC 8439, draft-irtf-cfrg-xchacha-03)
# ---------------------------------------------------------------------------------------------


def _rotl(v, c):
    return ((v << c) & 0xFFFFFFFF) | (v >> (32 - c))


def _quarter(s, a, b, c, d):
    s[a] = (s[a] + s[b]) & 0xFFFFFFFF
    s[d] = _rotl(s[d] ^ s[a], 16)
    s[c] = (s[c] + s[d]) & 0xFFFFFFFF
    s[b] = _rotl(s[b] ^ s[c], 12)
    s[a] = (s[a] + s[b]) & 0xFFFFFFFF
    s[d] = _rotl(s[d] ^ s[a], 8)
    s[c] = (s[c] + s[d]) & 0xFFFFFFFF
    s[b] = _rotl(s[b] ^ s[c], 7)


CONSTANTS = [0x61707865, 0x3320646E, 0x79622D32, 0x6B206574]


def _rounds(state):
    s = list(state)
    for _ in range(10):
        _quarter(s, 0, 4, 8, 12)
        _quarter(s, 1, 5, 9, 13)
        _quarter(s, 2, 6, 10, 14)
        _quarter(s, 3, 7, 11, 15)
        _quarter(s, 0, 5, 10, 15)
        _quarter(s, 1, 6, 11, 12)
        _quarter(s, 2, 7, 8, 13)
        _quarter(s, 3, 4, 9, 14)
    return s


def chacha20_block(key, counter, nonce):
    state = CONSTANTS + list(struct.unpack("<8I", key)) + [counter] + list(
        struct.unpack("<3I", nonce)
    )
    s = _rounds(state)
    return struct.pack("<16I", *[(a + b) & 0xFFFFFFFF for a, b in zip(s, state)])


def chacha20_xor(key, counter, nonce, data):
    out = bytearray()
    for i in range(0, len(data), 64):
        block = chacha20_block(key, counter + i // 64, nonce)
        out += bytes(a ^ b for a, b in zip(data[i : i + 64], block))
    return bytes(out)


def hchacha20(key, nonce16):
    state = CONSTANTS + list(struct.unpack("<8I", key)) + list(struct.unpack("<4I", nonce16))
    s = _rounds(state)
    return struct.pack("<8I", *(s[0:4] + s[12:16]))


def poly1305(key, msg):
    r = int.from_bytes(key[:16], "little") & 0x0FFFFFFC0FFFFFFC0FFFFFFC0FFFFFFF
    s = int.from_bytes(key[16:], "little")
    acc = 0
    p = (1 << 130) - 5
    for i in range(0, len(msg), 16):
        n = int.from_bytes(msg[i : i + 16] + b"\x01", "little")
        acc = (acc + n) * r % p
    return int.to_bytes((acc + s) & ((1 << 128) - 1), 16, "little")


def _pad16(data):
    return b"\x00" * (-len(data) % 16)


def chacha20poly1305_seal(key, nonce, plaintext, aad):
    otk = chacha20_block(key, 0, nonce)[:32]
    ct = chacha20_xor(key, 1, nonce, plaintext)
    mac = aad + _pad16(aad) + ct + _pad16(ct) + struct.pack("<QQ", len(aad), len(ct))
    return ct + poly1305(otk, mac)


def xchacha20poly1305_seal(key, nonce24, plaintext, aad):
    subkey = hchacha20(key, nonce24[:16])
    return chacha20poly1305_seal(subkey, b"\x00" * 4 + nonce24[16:], plaintext, aad)


# ---------------------------------------------------------------------------------------------
# HKDF (RFC 5869) and HPKE base mode (RFC 9180)
# ---------------------------------------------------------------------------------------------


def hkdf_extract(salt, ikm):
    return hmac.new(salt or b"\x00" * 32, ikm, hashlib.sha256).digest()


def hkdf_expand(prk, info, length):
    out, block, i = b"", b"", 1
    while len(out) < length:
        block = hmac.new(prk, block + info + bytes([i]), hashlib.sha256).digest()
        out += block
        i += 1
    return out[:length]


def hkdf(ikm, info, length):
    return hkdf_expand(hkdf_extract(b"", ikm), info, length)


KEM_SUITE = b"KEM" + struct.pack(">H", 0x0020)
HPKE_SUITE = b"HPKE" + struct.pack(">HHH", 0x0020, 0x0001, 0x0003)


def _labeled_extract(suite, salt, label, ikm):
    return hkdf_extract(salt, b"HPKE-v1" + suite + label + ikm)


def _labeled_expand(suite, prk, label, info, length):
    return hkdf_expand(prk, struct.pack(">H", length) + b"HPKE-v1" + suite + label + info, length)


def hpke_derive_keypair(ikm):
    prk = _labeled_extract(KEM_SUITE, b"", b"dkp_prk", ikm)
    sk = _labeled_expand(KEM_SUITE, prk, b"sk", b"", 32)
    return sk, x25519_public(sk)


def hpke_seal_base(pk_r, info, aad, plaintext, ikm_e):
    sk_e, pk_e = hpke_derive_keypair(ikm_e)
    dh = x25519(sk_e, pk_r)
    eae_prk = _labeled_extract(KEM_SUITE, b"", b"eae_prk", dh)
    shared = _labeled_expand(KEM_SUITE, eae_prk, b"shared_secret", pk_e + pk_r, 32)
    psk_id_hash = _labeled_extract(HPKE_SUITE, b"", b"psk_id_hash", b"")
    info_hash = _labeled_extract(HPKE_SUITE, b"", b"info_hash", info)
    context = b"\x00" + psk_id_hash + info_hash
    secret = _labeled_extract(HPKE_SUITE, shared, b"secret", b"")
    key = _labeled_expand(HPKE_SUITE, secret, b"key", context, 32)
    nonce = _labeled_expand(HPKE_SUITE, secret, b"base_nonce", context, 12)
    return pk_e, chacha20poly1305_seal(key, nonce, plaintext, aad)


# ---------------------------------------------------------------------------------------------
# Anchors: each primitive against its published vector
# ---------------------------------------------------------------------------------------------


def h(text):
    return bytes.fromhex(text)


def check_anchors():
    seed = h("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
    assert ed25519_public(seed) == h(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    ), "RFC 8032 test 1 public key"
    assert ed25519_sign(seed, b"") == h(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155"
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    ), "RFC 8032 test 1 signature"
    assert x25519(
        h("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
        h("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
    ) == h("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"), "RFC 7748 5.2"
    pk_r = h("4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a")
    assert x25519_public(
        h("8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb")
    ) == pk_r, "RFC 9180 A.2.1 pkRm"
    enc, ct = hpke_seal_base(
        pk_r,
        h("4f6465206f6e2061204772656369616e2055726e"),
        h("436f756e742d30"),
        h("4265617574792069732074727574682c20747275746820626561757479"),
        h("909a9b35d3dc4713a5e72a4da274b55d3d3821a37e5d099e74a647db583a904b"),
    )
    assert enc == h("1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a")
    assert ct == h(
        "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db"
        "21993c62ce81883d2dd1b51a28"
    ), "RFC 9180 A.2.1.1 sequence 0"
    out = xchacha20poly1305_seal(
        h("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f"),
        h("404142434445464748494a4b4c4d4e4f5051525354555657"),
        h(
            "4c616469657320616e642047656e746c656d656e206f662074686520636c6173"
            "73206f66202739393a204966204920636f756c64206f6666657220796f75206f"
            "6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73"
            "637265656e20776f756c642062652069742e"
        ),
        h("50515253c0c1c2c3c4c5c6c7"),
    )
    assert out.hex().endswith("c0875924c1c7987947deafd8780acf49"), "XChaCha draft A.3.1 tag"
    assert out.hex().startswith("bd6d179d3e83d43b9576579493c0e939"), "XChaCha draft A.3.1"
    for low in (
        "00" * 32,
        "01" + "00" * 31,
        "e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800",
        "5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157",
        "ec" + "ff" * 30 + "7f",
    ):
        assert x25519(counting(0x21, 32), h(low)) == b"\x00" * 32, "low order " + low
    okm = hkdf_expand(
        hkdf_extract(h("000102030405060708090a0b0c"), h("0b" * 22)),
        h("f0f1f2f3f4f5f6f7f8f9"),
        42,
    )
    assert okm == h(
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    ), "RFC 5869 test case 1"


# ---------------------------------------------------------------------------------------------
# The crate's formats
# ---------------------------------------------------------------------------------------------


def field(data):
    return struct.pack(">I", len(data)) + data


def framed(*parts):
    return b"".join(field(p) for p in parts)


def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def counting(start, length):
    return bytes((start + i) % 256 for i in range(length))


TEAM = b"team-vectors"
ISSUED_AT = 1_700_000_000_000
OWNER = (b"owner", b"laptop")


def safety_number(digest):
    return " ".join(
        "%05d" % (int.from_bytes(digest[5 * i : 5 * i + 5], "big") % 100000) for i in range(6)
    )


def entry_hash(index, prev, entry):
    return hashlib.sha256(
        framed(b"hangar-team/v1/memberlog", struct.pack(">Q", index), prev, entry)
    ).digest()


def statement(label, holder, sign_pub, kem_pub):
    """What a device's proof of possession and an endorsement sign (recovery flag 0)."""
    return framed(
        b"hangar-team/v1/" + label, TEAM, holder[0], holder[1], sign_pub, kem_pub, b"\x00"
    )


def entry_header(index, prev, issued_at, tag):
    return framed(
        b"hangar-team/v1/memberlog-entry",
        b"\x01",
        TEAM,
        struct.pack(">Q", index),
        prev,
        struct.pack(">Q", issued_at),
        OWNER[0],
        OWNER[1],
        bytes([tag]),
    )


ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"


def crockford(data):
    bits = "".join(format(b, "08b") for b in data)
    chars = [ALPHABET[int(bits[i : i + 5], 2)] for i in range(0, len(bits), 5)]
    return "-".join("".join(chars[i : i + 4]) for i in range(0, len(chars), 4))


def compute():
    out = {}
    seeds = counting(0x00, 64)
    sign_seed, kem_secret = seeds[:32], _clamp(seeds[32:])
    sign_pub, kem_pub = ed25519_public(sign_seed), x25519_public(kem_secret)
    out["VECTOR_OWNER_SIGN"] = sign_pub.hex()
    out["VECTOR_OWNER_KEM"] = kem_pub.hex()
    out["VECTOR_OWNER_SAFETY"] = safety_number(
        hashlib.sha256(
            framed(b"hangar-team/v1/safety-number", OWNER[0], OWNER[1], sign_pub, kem_pub)
        ).digest()
    )

    device = framed(
        OWNER[1],
        sign_pub,
        kem_pub,
        b"\x00",
        ed25519_sign(sign_seed, statement(b"device-proof", OWNER, sign_pub, kem_pub)),
    )
    create = entry_header(1, b"", ISSUED_AT, 1) + field(struct.pack(">I", 1)) + device
    create_entry = create + ed25519_sign(sign_seed, create)
    create_hash = entry_hash(1, b"", create_entry)
    out["VECTOR_CREATE_ENTRY"] = b64(create_entry)
    out["VECTOR_CREATE_HASH"] = create_hash.hex()

    secret = counting(0x40, 32)
    commitment = hashlib.sha256(
        framed(b"hangar-team/v1/epoch-commitment", TEAM, struct.pack(">I", 1), secret)
    ).digest()
    out["VECTOR_COMMITMENT"] = commitment.hex()
    rotate = (
        entry_header(2, create_hash, ISSUED_AT + 1, 7)
        + framed(struct.pack(">I", 1), commitment, b"")
    )
    rotate_entry = rotate + ed25519_sign(sign_seed, rotate)
    rotate_hash = entry_hash(2, create_hash, rotate_entry)
    out["VECTOR_ROTATE_ENTRY"] = b64(rotate_entry)
    out["VECTOR_ROTATE_HASH"] = rotate_hash.hex()

    # Entry 3: the owner's laptop adds the owner's phone (its own proof, the laptop's
    # endorsement).
    phone_seeds = counting(0x60, 64)
    phone_seed = phone_seeds[:32]
    phone_sign, phone_kem = ed25519_public(phone_seed), x25519_public(_clamp(phone_seeds[32:]))
    phone = (OWNER[0], b"phone")
    add = (
        entry_header(3, rotate_hash, ISSUED_AT + 2, 3)
        + field(OWNER[0])
        + framed(
            phone[1],
            phone_sign,
            phone_kem,
            b"\x00",
            ed25519_sign(phone_seed, statement(b"device-proof", phone, phone_sign, phone_kem)),
        )
        + framed(
            OWNER[1],
            ed25519_sign(sign_seed, statement(b"device-endorse", phone, phone_sign, phone_kem)),
        )
    )
    add_entry = add + ed25519_sign(sign_seed, add)
    add_hash = entry_hash(3, rotate_hash, add_entry)
    out["VECTOR_ADD_DEVICE_ENTRY"] = b64(add_entry)
    out["VECTOR_ADD_DEVICE_HASH"] = add_hash.hex()

    info = framed(b"hangar-team/v1/wrap", TEAM, struct.pack(">I", 1), OWNER[0], OWNER[1])
    enc, ct = hpke_seal_base(kem_pub, info, b"", secret, counting(0x80, 32))
    out["VECTOR_WRAP"] = b64(b"\x01" + enc + ct)

    new_secret = counting(0xA0, 32)
    history_key = hkdf(
        new_secret, framed(b"hangar-team/v1/history-key", TEAM, struct.pack(">I", 2)), 32
    )
    nonce = counting(0xC0, 24)
    sealed = xchacha20poly1305_seal(
        history_key,
        nonce,
        secret,
        framed(b"hangar-team/v1/history", TEAM, struct.pack(">I", 2)),
    )
    out["VECTOR_HISTORY"] = (b"\x01" + nonce + sealed).hex()

    # hangar-team-protocol EpochKeys: the sealing key of epoch 1.
    # hangar-team-protocol EpochKeys binds the log hash of the epoch's rotate entry.
    seal_key = hkdf(
        secret,
        framed(b"hangar-team/v1/seal-key", TEAM, struct.pack(">I", 1), rotate_hash),
        32,
    )
    head = struct.pack(">Q", 3), add_hash
    counter = struct.pack(">Q", 7)
    aad = framed(
        b"hangar-team/v1/record",
        b"\x01",
        TEAM,
        struct.pack(">I", 1),
        b"reservation-note",
        b"task-hash",
        OWNER[0],
        OWNER[1],
        head[0],
        head[1],
        counter,
    )
    nonce = counting(0xE0, 24)
    ciphertext = xchacha20poly1305_seal(seal_key, nonce, b"hello team", aad)
    signature = ed25519_sign(
        sign_seed, framed(b"hangar-team/v1/record-sig", aad, nonce, ciphertext)
    )
    out["VECTOR_RECORD"] = b64(
        b"\x01" + head[0] + head[1] + counter + nonce + ciphertext + signature
    )

    entropy = counting(0x10, 16)
    check = hashlib.sha256(framed(b"hangar-team/v1/recovery-code", b"\x01", entropy)).digest()
    out["VECTOR_RECOVERY_CODE"] = crockford(b"\x01" + entropy + check[:3])
    okm = hkdf(entropy, framed(b"hangar-team/v1/recovery-identity"), 64)
    out["VECTOR_RECOVERY_SIGN"] = ed25519_public(okm[:32]).hex()
    out["VECTOR_RECOVERY_KEM"] = x25519_public(okm[32:]).hex()
    return out


def main():
    check_anchors()
    print(
        "anchors: RFC 8032, RFC 7748, RFC 9180 A.2.1, XChaCha draft A.3.1, RFC 5869, "
        "X25519 low-order points ok"
    )
    source = SRC.read_text(encoding="utf-8")
    failed = False
    for name, value in compute().items():
        match = re.search(r"const %s: &str =\s*\"([^\"]*)\";" % name, source)
        print("%s = %s" % (name, value))
        if match is None:
            print("  missing in src/tests.rs")
            failed = True
        elif match.group(1) != value:
            print("  differs from src/tests.rs: %s" % match.group(1))
            failed = True
    if failed:
        sys.exit(1)
    print("all vectors match")


if __name__ == "__main__":
    main()
