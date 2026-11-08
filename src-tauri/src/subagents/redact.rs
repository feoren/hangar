//! Masks what looks like a secret in assistant text before it leaves the sub-agent source.
//!
//! A best-effort filter, not a guarantee: it catches the common shapes an agent prints when it
//! reads a `.env` file, a config or a command line (`API_KEY=...`, `"token": "..."`,
//! `password: ...`, `Authorization: Bearer ...`, `https://user:pass@host`), well-known token
//! prefixes and PEM private keys. Over-masking is preferred to a leak.

pub const REDACTED: &str = "[redacted]";

/// Names whose value is masked when they end in one of these (case-insensitive): `API_KEY`,
/// `GITHUB_TOKEN`, `client_secret`, `password`.
const SECRET_NAMES: &[&str] = &["key", "token", "secret", "password", "passwd", "pwd"];

/// Token prefixes and the least number of characters that must follow them, so that ordinary
/// words such as `sk-learn` stay.
const PREFIXES: &[(&str, usize)] = &[
    ("sk-", 16),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghu_", 20),
    ("ghs_", 20),
    ("ghr_", 20),
    ("github_pat_", 20),
    ("glpat-", 20),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("xoxa-", 10),
    ("xoxr-", 10),
    ("xoxs-", 10),
    ("xapp-", 10),
    ("AKIA", 16),
    ("AIza", 24),
    ("sk_live_", 16),
    ("sk_test_", 16),
    ("rk_live_", 16),
    ("npm_", 24),
    ("hf_", 24),
    // A JSON Web Token: base64 of `{"`.
    ("eyJ", 24),
];

/// Characters a token is made of (base64, base64url, hex and the separators of the formats above).
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'+' | b'=')
}

/// A token found after `Bearer ` needs at least this many characters, so prose stays.
const MIN_BEARER: usize = 16;
/// A user name without a password in a URL is masked from this length on (a token used as user).
const MIN_URL_USER: usize = 16;

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const DASHES: &str = "-----";

pub fn redact(text: &str) -> String {
    let text = redact_pem(text);
    let text = redact_url_credentials(&text);
    let text = redact_bearer(&text);
    let text = redact_assignments(&text);
    redact_prefixes(&text)
}

/// `-----BEGIN ... KEY-----` (or `... PRIVATE KEY BLOCK-----`, PGP) up to the matching end line
/// (or the end of the text, when cut).
fn redact_pem(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(begin) = rest.find(PEM_BEGIN) {
        let after = &rest[begin + PEM_BEGIN.len()..];
        let Some(label_end) = after.find(DASHES) else {
            break;
        };
        let label = &after[..label_end];
        if !(label.ends_with("KEY") || label.contains("PRIVATE KEY")) {
            out.push_str(&rest[..begin + PEM_BEGIN.len()]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..begin]);
        out.push_str(REDACTED);
        rest = match after.find(PEM_END) {
            Some(end) => {
                let tail = &after[end + PEM_END.len()..];
                tail.find(DASHES)
                    .map_or("", |close| &tail[close + DASHES.len()..])
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// `NAME=value`, `NAME = "value"`, `"name": "value"` and `name: value` (YAML, HTTP headers such as
/// `x-api-key: ...`) with a secret-looking name. A bare `key:` is left alone: in prose it is far
/// more often a heading than a secret.
fn redact_assignments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !is_name_byte(bytes[i]) || (i > 0 && is_name_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_name_byte(bytes[i]) {
            i += 1;
        }
        let name = text[start..i].to_ascii_lowercase();
        if !SECRET_NAMES.iter().any(|s| name.ends_with(s)) {
            continue;
        }
        // `"name"`: the same quote on both sides (`'x-api-key: ...'` is a quoted header, not a
        // quoted name).
        let quoted_name = start > 0
            && matches!(bytes[start - 1], b'"' | b'\'')
            && bytes.get(i) == Some(&bytes[start - 1]);
        let mut j = if quoted_name { i + 1 } else { i };
        while j < bytes.len() && bytes[j] == b' ' {
            j += 1;
        }
        let Some(&separator) = bytes.get(j) else {
            break;
        };
        let colon_allowed = quoted_name || name != "key";
        if separator != b'=' && !(separator == b':' && colon_allowed) {
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j] == b' ' {
            j += 1;
        }
        let (value_start, value_end) = match bytes.get(j) {
            Some(&quote @ (b'"' | b'\'')) => {
                let end = text[j + 1..]
                    .find(char::from(quote))
                    .map_or(text.len(), |e| j + 1 + e);
                (j + 1, end)
            }
            // A JSON name needs a quoted value; `"key": 5` or `"key": {` is no secret.
            Some(_) if separator == b':' && quoted_name => continue,
            // Unquoted: up to a separator or the quote that closes a surrounding argument
            // (`-H 'x-api-key: ...'`).
            Some(_) => {
                let end = text[j..]
                    .find(|c: char| {
                        c.is_whitespace() || matches!(c, ',' | ';' | '&' | '}' | ')' | '"' | '\'')
                    })
                    .map_or(text.len(), |e| j + e);
                (j, end)
            }
            None => continue,
        };
        if value_end > value_start {
            out.push_str(&text[copied..value_start]);
            out.push_str(REDACTED);
            copied = value_end;
        }
        i = value_end.max(i);
    }
    out.push_str(&text[copied..]);
    out
}

/// `https://user:password@host`: the password; a long user name without one (a token used as
/// user, `https://ghp_...@github.com`) as a whole.
fn redact_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let authority_start = scheme_end + 3;
        let after = &rest[authority_start..];
        let authority_len = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'' | '<' | '>' | '`')
            })
            .unwrap_or(after.len());
        let authority = &after[..authority_len];
        let masked = authority.rfind('@').and_then(|at| {
            let user_info = &authority[..at];
            match user_info.find(':') {
                Some(colon) if colon + 1 < at => Some((colon + 1, at)),
                None if user_info.len() >= MIN_URL_USER => Some((0, at)),
                _ => None,
            }
        });
        out.push_str(&rest[..authority_start]);
        match masked {
            Some((from, to)) => {
                out.push_str(&authority[..from]);
                out.push_str(REDACTED);
                out.push_str(&authority[to..]);
            }
            None => out.push_str(authority),
        }
        rest = &after[authority_len..];
    }
    out.push_str(rest);
    out
}

/// `Bearer <token>` (any case), as in an `Authorization` header.
fn redact_bearer(text: &str) -> String {
    const BEARER: &str = "bearer ";
    // ASCII lower case keeps every byte offset.
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = lower[from..].find(BEARER) {
        let start = from + found;
        let mut token = start + BEARER.len();
        from = token;
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        while bytes.get(token) == Some(&b' ') {
            token += 1;
        }
        let len = bytes[token..]
            .iter()
            .take_while(|b| is_token_byte(**b))
            .count();
        if len >= MIN_BEARER {
            out.push_str(&text[copied..token]);
            out.push_str(REDACTED);
            copied = token + len;
            from = copied;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Words that start with a known token prefix. The word may follow a separator such as `/`,
/// `=` or `-` (a URL, `--token=ghp_...`), but not a letter, digit or `_`.
fn redact_prefixes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i += 1;
            continue;
        }
        let hit = PREFIXES.iter().find(|(prefix, min)| {
            // Bytes, not `str`: `i` may sit inside a multi-byte character.
            bytes[i..].starts_with(prefix.as_bytes()) && {
                let body = bytes[i + prefix.len()..]
                    .iter()
                    .take_while(|b| is_token_byte(**b))
                    .count();
                body >= *min
            }
        });
        if hit.is_some() {
            let end = i + bytes[i..].iter().take_while(|b| is_token_byte(**b)).count();
            out.push_str(&text[copied..i]);
            out.push_str(REDACTED);
            copied = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Harmless markers that only have the shape of a secret.
    const FAKE: &str = "FAKEVALUE0123456789abcdefFAKE";

    #[test]
    fn assignments_in_env_and_json_form() {
        for (input, expected) in [
            (format!("API_KEY={FAKE}"), "API_KEY=[redacted]".to_owned()),
            (
                format!("export github_token = {FAKE}; next"),
                "export github_token = [redacted]; next".to_owned(),
            ),
            (
                format!("Password='{FAKE}' ok"),
                "Password='[redacted]' ok".to_owned(),
            ),
            (
                format!("{{\"client_secret\": \"{FAKE}\", \"n\": 1}}"),
                "{\"client_secret\": \"[redacted]\", \"n\": 1}".to_owned(),
            ),
            (
                format!("{{'apiKey':'{FAKE}'}}"),
                "{'apiKey':'[redacted]'}".to_owned(),
            ),
            (
                format!("SECRET={FAKE}&a=b"),
                "SECRET=[redacted]&a=b".to_owned(),
            ),
        ] {
            assert_eq!(redact(&input), expected);
        }
        // Prose, other names and non-string JSON values stay.
        for same in [
            "The key: we need to look at the parser.",
            "count=3 and \"token\": 12",
            "monkey business",
            "a == b",
            "\"key\": {",
        ] {
            assert_eq!(redact(same), same);
        }
    }

    #[test]
    fn known_prefixes() {
        for prefix in [
            "sk-",
            "sk-ant-api03-",
            "sk-proj-",
            "sk_live_",
            "ghp_",
            "ghr_",
            "glpat-",
            "AIza",
            "xapp-",
            "eyJ",
            "github_pat_",
            "gho_",
            "AKIA",
            "xoxb-",
            "xoxp-",
        ] {
            let input = format!("use {prefix}{FAKE} now");
            assert_eq!(redact(&input), "use [redacted] now", "{prefix}");
        }
        for same in ["pip install scikit-learn sk-learn", "a task-sk-x", "AKIA"] {
            assert_eq!(redact(same), same);
        }
    }

    #[test]
    fn headers_urls_and_yaml() {
        for (input, expected) in [
            (
                format!("curl -H \"Authorization: Bearer {FAKE}\" x"),
                "curl -H \"Authorization: Bearer [redacted]\" x".to_owned(),
            ),
            (
                format!("-H 'x-api-key: {FAKE}'"),
                "-H 'x-api-key: [redacted]'".to_owned(),
            ),
            (
                format!(
                    "password: {FAKE}
user: me"
                ),
                "password: [redacted]
user: me"
                    .to_owned(),
            ),
            (
                format!("DATABASE_URL=postgres://app:{FAKE}@db:5432/x"),
                "DATABASE_URL=postgres://app:[redacted]@db:5432/x".to_owned(),
            ),
            (
                format!("git clone https://ghp_{FAKE}@github.com/a/b"),
                "git clone https://[redacted]@github.com/a/b".to_owned(),
            ),
            (
                format!("cmd --api=sk-{FAKE} /AKIA{FAKE}"),
                "cmd --api=[redacted] /[redacted]".to_owned(),
            ),
        ] {
            assert_eq!(redact(&input), expected);
        }
        for same in [
            "the bearer of bad news",
            "see https://example.com/path?q=1 and ssh://git@example.com",
            "Bearer tokens expire",
        ] {
            assert_eq!(redact(same), same);
        }
    }

    #[test]
    fn pem_keys_up_to_their_end_line() {
        let input = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAFAKE\n-----END OPENSSH PRIVATE KEY-----\nafter";
        assert_eq!(redact(input), "before\n[redacted]\nafter");
        // Cut off by the excerpt limit: everything after the begin line goes.
        assert_eq!(
            redact("x -----BEGIN RSA PRIVATE KEY-----\nAAAA"),
            "x [redacted]"
        );
        let pgp = "-----BEGIN PGP PRIVATE KEY BLOCK-----
AAAA
-----END PGP PRIVATE KEY BLOCK-----";
        assert_eq!(redact(pgp), "[redacted]");
        let cert = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----";
        assert_eq!(redact(cert), cert);
    }

    #[test]
    fn never_panics_on_mixed_text() {
        let parts = [
            "ä",
            "\u{202e}",
            "中",
            "sk-",
            "KEY",
            "=",
            "\"",
            "'",
            ":",
            " ",
            "-----BEGIN ",
            "KEY-----",
            "-----END ",
            "ghp_",
            "x",
            "AKIA",
            "://",
            "@",
            "Bearer ",
        ];
        for a in parts {
            for b in parts {
                for c in parts {
                    let _ = redact(&format!("{a}{b}{c}{a}{FAKE}{b}"));
                }
            }
        }
    }

    #[test]
    fn non_ascii_text_is_kept_intact() {
        let input = format!("naïve café TOKEN={FAKE} 中文");
        assert_eq!(redact(&input), "naïve café TOKEN=[redacted] 中文");
    }
}
