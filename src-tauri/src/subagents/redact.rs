//! Masks what looks like a secret in transcript text before it leaves the sub-agent source.
//!
//! A best-effort filter, not a guarantee: it catches the common shapes an agent prints when it
//! reads a `.env` file, a config or a command line (`API_KEY=...`, `"token": "..."`,
//! `password: ...`, `--password ...`, `sshpass -p ...`, `curl -u user:...`,
//! `Authorization: Bearer|Basic|token ...`, `https://user:pass@host`,
//! `ConvertTo-SecureString "..." -AsPlainText`), well-known token prefixes and PEM private keys.
//! Over-masking is preferred to a leak. Callers strip terminal sequences first
//! ([`strip_controls`]), so a color code cannot split a name from its value.

pub const REDACTED: &str = "[redacted]";

/// Names whose value is masked when they end in one of these (case-insensitive): `API_KEY`,
/// `GITHUB_TOKEN`, `client_secret`, `password`, `--passphrase`, `aws_credentials`.
const SECRET_SUFFIXES: &[&str] = &[
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "credential",
    "credentials",
];

/// Short words that are secrets only as a whole name or its last part after `_` or `-`
/// (`DB_PASS`, npm `_auth`, `Set-Cookie`), so `compass` or `oauth2` stay.
const SECRET_WORDS: &[&str] = &["pass", "auth", "cookie"];

/// Bare names that are far more often a heading in prose than a secret: an unquoted
/// `name: value` masks only a value that looks like one (long, or holding `=`).
const LOOSE_NAMES: &[&str] = &["key", "auth", "cookie"];
/// A value after a loose name needs this many token characters to count as a secret.
const MIN_LOOSE_VALUE: usize = 16;

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
    // SendGrid, DigitalOcean, Shopify, Google OAuth access tokens.
    ("SG.", 20),
    ("dop_v1_", 20),
    ("shpat_", 20),
    ("ya29.", 20),
    // A JSON Web Token: base64 of `{"`.
    ("eyJ", 24),
];

/// Characters a token is made of (base64, base64url, hex and the separators of the formats above).
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'+' | b'=')
}

/// A token found after `Bearer ` anywhere needs at least this many characters, so prose stays.
const MIN_BEARER: usize = 16;
/// After `Authorization:` the scheme says it is a credential; shorter ones count too.
const MIN_AUTHORIZATION: usize = 8;
/// A user name without a password in a URL is masked from this length on (a token used as user).
const MIN_URL_USER: usize = 16;

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const DASHES: &str = "-----";

pub fn redact(text: &str) -> String {
    let text = redact_pem(text);
    let text = redact_url_credentials(&text);
    let text = redact_authorization(&text);
    let text = redact_bearer(&text);
    let text = redact_secure_string(&text);
    let text = redact_tool_flags(&text);
    let text = redact_assignments(&text);
    redact_prefixes(&text)
}

/// Removes terminal escape sequences (CSI such as colors, OSC such as titles and links, DCS and
/// the other string sequences, two-byte escapes, their 8-bit forms), every other control
/// character except newline and tab, and invisible format characters
/// ([`crate::text::is_invisible`]: bidi overrides, zero-width characters).
pub fn strip_controls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    skip_csi(&mut chars);
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    chars.next();
                    skip_string(&mut chars);
                }
                // `ESC ( B` and the like: intermediates, then one final byte.
                Some(next) if ('\u{20}'..='\u{2f}').contains(&next) => {
                    while chars
                        .peek()
                        .is_some_and(|c| ('\u{20}'..='\u{2f}').contains(c))
                    {
                        chars.next();
                    }
                    if chars
                        .peek()
                        .is_some_and(|c| ('\u{30}'..='\u{7e}').contains(c))
                    {
                        chars.next();
                    }
                }
                Some(next) if ('\u{30}'..='\u{7e}').contains(&next) => {
                    chars.next();
                }
                _ => {}
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => skip_string(&mut chars),
            '\n' | '\t' => out.push(c),
            c if c.is_control() || crate::text::is_invisible(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Parameters and intermediates (`0x20..=0x3F`), then one final byte (`0x40..=0x7E`).
fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars
        .peek()
        .is_some_and(|c| ('\u{20}'..='\u{3f}').contains(c))
    {
        chars.next();
    }
    if chars
        .peek()
        .is_some_and(|c| ('\u{40}'..='\u{7e}').contains(c))
    {
        chars.next();
    }
}

/// Up to BEL, `ESC \` or the 8-bit string terminator; an unterminated string runs to the end.
fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(c) = chars.next() {
        match c {
            '\u{7}' | '\u{9c}' => return,
            '\u{1b}' if chars.peek() == Some(&'\\') => {
                chars.next();
                return;
            }
            _ => {}
        }
    }
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

/// Replaces the given byte ranges (sorted, not overlapping, on character boundaries).
fn mask_ranges(text: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for &(start, end) in ranges {
        if start < copied || end <= start {
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(REDACTED);
        copied = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// A secret-looking name, compared in lower case and without the dashes of a command line flag.
fn is_secret_name(lower: &str) -> bool {
    let bare = lower.trim_start_matches('-');
    SECRET_SUFFIXES.iter().any(|s| bare.ends_with(s))
        || SECRET_WORDS.iter().any(|word| {
            bare == *word
                || bare
                    .strip_suffix(word)
                    .is_some_and(|head| head.ends_with('_') || head.ends_with('-'))
        })
}

/// The unquoted value starting at `start`: up to whitespace, a separator or the quote that closes
/// a surrounding argument (`-H 'x-api-key: ...'`).
fn unquoted_end(text: &str, start: usize) -> usize {
    text[start..]
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '&' | '}' | ')' | '"' | '\''))
        .map_or(text.len(), |e| start + e)
}

/// `(start, end)` of the value at `j`: the inside of a quoted value (a backslash escapes the next
/// byte, so `"a\"b"` is one value), else the unquoted word.
fn value_at(text: &str, j: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    match bytes.get(j) {
        Some(&quote @ (b'"' | b'\'')) => {
            let mut at = j + 1;
            while at < bytes.len() && bytes[at] != quote {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            // The quote is ASCII, so `at` is a character boundary or the end.
            Some((j + 1, at.min(bytes.len())))
        }
        Some(_) => Some((j, unquoted_end(text, j))),
        None => None,
    }
}

/// The first byte at or after `at` that is not whitespace on the same line (spaces, tabs,
/// no-break spaces and the other Unicode spaces).
fn skip_spaces(text: &str, at: usize) -> usize {
    text[at..]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace() || *c == '\n')
        .map_or(text.len(), |(offset, _)| at + offset)
}

/// `NAME=value`, `NAME = "value"`, `"name": "value"`, `name: value` (YAML, HTTP headers such as
/// `x-api-key: ...`) and command line flags `--password value`, `--client-secret=value` with a
/// secret-looking name. `Cookie:` headers are masked to the end of the line.
fn redact_assignments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
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
        if !is_secret_name(&name) {
            continue;
        }
        // `"name"`: the same quote on both sides (`'x-api-key: ...'` is a quoted header, not a
        // quoted name).
        let quoted_name = start > 0
            && matches!(bytes[start - 1], b'"' | b'\'')
            && bytes.get(i) == Some(&bytes[start - 1]);
        let flag = name.starts_with('-') && !name.starts_with("--no-");
        let mut j = skip_spaces(text, if quoted_name { i + 1 } else { i });
        let Some(&separator) = bytes.get(j) else {
            break;
        };
        let spaced = j > i;
        if separator == b'=' || separator == b':' {
            j = skip_spaces(text, j + 1);
        } else if !(flag && spaced && separator != b'-' && separator != b'\n') {
            // `--password value`: a flag takes the next word; `--password --other` has none.
            continue;
        }
        let Some((value_start, mut value_end)) = value_at(text, j) else {
            continue;
        };
        let quoted_value = value_start > j;
        // A JSON name needs a quoted value; `"key": 5` or `"key": {` is no secret.
        // Skipped values are passed over as a whole: resuming right after the name would scan
        // the same long value again for every name in it (`key:key:key:...`).
        if separator == b':' && quoted_name && !quoted_value {
            i = value_end.max(i);
            continue;
        }
        let bare = name.trim_start_matches('-');
        if separator == b':' && !quoted_name && LOOSE_NAMES.contains(&bare) && !quoted_value {
            let value = &text[value_start..value_end];
            let token_len = value.bytes().take_while(|b| is_token_byte(*b)).count();
            if token_len < MIN_LOOSE_VALUE && !value.contains('=') {
                i = value_end.max(i);
                continue;
            }
        }
        if bare.ends_with("cookie") && !quoted_value {
            // `Cookie: a=1; b=2` holds several values: the whole rest of the line.
            value_end = text[value_start..]
                .find(['\n', '"', '\''])
                .map_or(text.len(), |e| value_start + e);
        }
        if value_end > value_start {
            ranges.push((value_start, value_end));
        }
        i = value_end.max(i);
    }
    mask_ranges(text, &ranges)
}

/// Byte ranges of the whitespace-separated words of one line. A quote opened at the start of a
/// word, after `=` or inside a flag (`-p'a b'`) keeps its spaces in the word, as a shell would;
/// an apostrophe inside a plain word (`it's`) opens nothing.
fn words(line: &str, offset: usize) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, c) in line.char_indices() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
        } else if c.is_whitespace() {
            if let Some(s) = start.take() {
                found.push((offset + s, offset + index));
            }
        } else {
            let word_start = *start.get_or_insert(index);
            if matches!(c, '"' | '\'')
                && (word_start == index || previous == '=' || line[word_start..].starts_with('-'))
            {
                quote = Some(c);
            }
        }
        previous = c;
    }
    if let Some(s) = start {
        found.push((offset + s, offset + line.len()));
    }
    found
}

/// The program a word names: its file name in lower case without `.exe`.
fn program(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let lower = name.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
}

/// A quoted word's inside, else the word.
fn unquote(text: &str, (start, end): (usize, usize)) -> (usize, usize) {
    let word = &text.as_bytes()[start..end];
    if word.len() >= 2 && matches!(word[0], b'"' | b'\'') && word[word.len() - 1] == word[0] {
        (start + 1, end - 1)
    } else {
        (start, end)
    }
}

/// Short password flags of tools that take one there (`sshpass -p X`, `docker login -p X`,
/// `mysql -pX`; `mysql -p db` prompts and names a database, `docker run -p 80:80` is a port),
/// and `-u` or `--user` with `user:password` (curl): everything after the first `:`, unless both
/// sides are numbers (`docker run -u 1000:1000`).
fn redact_tool_flags(text: &str) -> String {
    const MYSQL: &[&str] = &["mysql", "mysqldump", "mysqladmin", "mysqlimport", "mariadb"];
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let list = words(line, offset);
        offset += line.len();
        let mut tool = String::new();
        for (index, &(start, end)) in list.iter().enumerate() {
            let word = &text[start..end];
            let next = list.get(index + 1).copied();
            match word {
                "-p" => {
                    let takes =
                        tool == "sshpass" || tool == "docker login" || tool == "podman login";
                    if let (true, Some(value)) = (takes, next) {
                        ranges.push(unquote(text, value));
                    }
                }
                "-u" | "--user" => {
                    if let Some(value) = next {
                        ranges.extend(user_password(text, unquote(text, value)));
                    }
                }
                _ if word.starts_with("--user=") => {
                    let value = unquote(text, (start + "--user=".len(), end));
                    ranges.extend(user_password(text, value));
                }
                // `curl -uuser:password`.
                _ if word.len() > 2 && word.starts_with("-u") && !word.starts_with("--") => {
                    ranges.extend(user_password(text, unquote(text, (start + 2, end))));
                }
                _ if word.len() > 2 && word.starts_with("-p") && !word.starts_with("--") => {
                    if tool == "sshpass"
                        || tool == "docker login"
                        || tool == "podman login"
                        || MYSQL.contains(&tool.as_str())
                    {
                        ranges.push(unquote(text, (start + 2, end)));
                    }
                }
                _ if !word.starts_with('-') => {
                    let name = program(word);
                    if (tool == "docker" || tool == "podman") && name == "login" {
                        tool = format!("{tool} login");
                    } else if name == "sshpass"
                        || name == "docker"
                        || name == "podman"
                        || MYSQL.contains(&name.as_str())
                    {
                        tool = name;
                    }
                }
                _ => {}
            }
        }
    }
    ranges.sort_unstable();
    mask_ranges(text, &ranges)
}

fn user_password(text: &str, (start, end): (usize, usize)) -> Option<(usize, usize)> {
    let value = &text[start..end];
    let colon = value.find(':')?;
    let (user, password) = (&value[..colon], &value[colon + 1..]);
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if password.is_empty() || (numeric(user) && numeric(password)) {
        return None;
    }
    Some((start + colon + 1, end))
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

/// Length of the token at `at`, after skipping spaces; `(start, len)`.
fn token_at(bytes: &[u8], mut at: usize) -> (usize, usize) {
    while bytes.get(at) == Some(&b' ') {
        at += 1;
    }
    let len = bytes
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| is_token_byte(**b))
        .count();
    (at, len)
}

/// `Authorization: Basic|Bearer|Token|Digest <credential>` (header or `"Authorization": "..."`):
/// the scheme says it is a credential, so a shorter one counts than for a bare `Bearer`.
fn redact_authorization(text: &str) -> String {
    const NAME: &str = "authorization";
    const SCHEMES: &[&str] = &["basic ", "bearer ", "token ", "digest "];
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(NAME) {
        let mut at = from + found + NAME.len();
        from = at;
        if bytes.get(at) == Some(&b'"') || bytes.get(at) == Some(&b'\'') {
            at += 1;
        }
        while bytes.get(at) == Some(&b' ') {
            at += 1;
        }
        if !matches!(bytes.get(at), Some(b':' | b'=')) {
            continue;
        }
        at += 1;
        while matches!(bytes.get(at), Some(b' ' | b'"' | b'\'')) {
            at += 1;
        }
        let (start, len) = match SCHEMES.iter().find(|s| lower[at..].starts_with(**s)) {
            Some(scheme) => token_at(bytes, at + scheme.len()),
            // A raw key without a scheme (`Authorization: <key>`). Plain words (`required`)
            // stay: a key has a digit, a symbol or the length of a token.
            None => {
                let (start, len) = token_at(bytes, at);
                let word = &bytes[start..start + len];
                let looks_like_key =
                    len >= MIN_BEARER || word.iter().any(|b| !b.is_ascii_alphabetic());
                if !looks_like_key {
                    continue;
                }
                (start, len)
            }
        };
        if len >= MIN_AUTHORIZATION {
            ranges.push((start, start + len));
            from = start + len;
        }
    }
    mask_ranges(text, &ranges)
}

/// `Bearer <token>` (any case) anywhere, as in an `Authorization` header that was cut.
fn redact_bearer(text: &str) -> String {
    const BEARER: &str = "bearer ";
    // ASCII lower case keeps every byte offset.
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(BEARER) {
        let start = from + found;
        from = start + BEARER.len();
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let (token, len) = token_at(bytes, from);
        if len >= MIN_BEARER && !text[token..].starts_with(REDACTED) {
            ranges.push((token, token + len));
            from = token + len;
        }
    }
    mask_ranges(text, &ranges)
}

/// PowerShell `ConvertTo-SecureString "value" -AsPlainText` (also with `-String`): the value.
fn redact_secure_string(text: &str) -> String {
    const NAME: &str = "convertto-securestring";
    let lower = text.to_ascii_lowercase();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(NAME) {
        let mut at = from + found + NAME.len();
        from = at;
        let skip_spaces = |mut at: usize| {
            while lower.as_bytes().get(at) == Some(&b' ') {
                at += 1;
            }
            at
        };
        at = skip_spaces(at);
        if lower[at..].starts_with("-string ") {
            at = skip_spaces(at + "-string ".len());
        }
        if lower.as_bytes().get(at) == Some(&b'-') {
            continue;
        }
        if let Some((start, end)) = value_at(text, at) {
            if end > start {
                ranges.push((start, end));
                from = end;
            }
        }
    }
    mask_ranges(text, &ranges)
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

    fn masks(cases: &[(String, &str)]) {
        for (input, expected) in cases {
            assert_eq!(redact(input), *expected, "{input}");
        }
    }

    fn keeps(cases: &[&str]) {
        for same in cases {
            assert_eq!(redact(same), *same);
        }
    }

    #[test]
    fn assignments_in_env_and_json_form() {
        masks(&[
            (format!("API_KEY={FAKE}"), "API_KEY=[redacted]"),
            (
                format!("export github_token = {FAKE}; next"),
                "export github_token = [redacted]; next",
            ),
            (format!("Password='{FAKE}' ok"), "Password='[redacted]' ok"),
            (
                format!("{{\"client_secret\": \"{FAKE}\", \"n\": 1}}"),
                "{\"client_secret\": \"[redacted]\", \"n\": 1}",
            ),
            (format!("{{'apiKey':'{FAKE}'}}"), "{'apiKey':'[redacted]'}"),
            (format!("SECRET={FAKE}&a=b"), "SECRET=[redacted]&a=b"),
        ]);
        // Prose, other names and non-string JSON values stay.
        keeps(&[
            "The key: we need to look at the parser.",
            "count=3 and \"token\": 12",
            "monkey business",
            "a == b",
            "\"key\": {",
        ]);
    }

    #[test]
    fn more_secret_names() {
        masks(&[
            (format!("DB_PASS={FAKE}"), "DB_PASS=[redacted]"),
            (format!("pass: {FAKE}"), "pass: [redacted]"),
            (
                format!("gpg_passphrase={FAKE}"),
                "gpg_passphrase=[redacted]",
            ),
            (
                format!("AWS_CREDENTIALS={FAKE}"),
                "AWS_CREDENTIALS=[redacted]",
            ),
            (
                format!("//registry.example/:_auth={FAKE}"),
                "//registry.example/:_auth=[redacted]",
            ),
            (
                format!("{{\"auth\": \"{FAKE}\"}}"),
                "{\"auth\": \"[redacted]\"}",
            ),
            (format!("auth={FAKE}"), "auth=[redacted]"),
            (format!("auth: {FAKE}"), "auth: [redacted]"),
            (
                format!("Cookie: session={FAKE}; theme=dark\nnext"),
                "Cookie: [redacted]\nnext",
            ),
            (format!("Set-Cookie: sid={FAKE}"), "Set-Cookie: [redacted]"),
        ]);
        keeps(&[
            "Please pass the value along and bypass the cache.",
            "compass: north",
            "basic auth: use the token from the vault",
            "the cookie: chocolate chip",
            "oauth2 flows and the author: me",
            "credentials are stored in the keychain",
        ]);
    }

    #[test]
    fn command_line_flags() {
        masks(&[
            (
                format!("app --password {FAKE} --verbose"),
                "app --password [redacted] --verbose",
            ),
            (
                format!("app --client-secret \"{FAKE}\""),
                "app --client-secret \"[redacted]\"",
            ),
            (
                format!("gh auth --token={FAKE}"),
                "gh auth --token=[redacted]",
            ),
            (
                format!("tool --api-key {FAKE}"),
                "tool --api-key [redacted]",
            ),
            (
                format!("sshpass -p {FAKE} ssh host"),
                "sshpass -p [redacted] ssh host",
            ),
            (
                format!("sshpass -p'{FAKE}' ssh host"),
                "sshpass -p'[redacted]' ssh host",
            ),
            (
                format!("docker login -u me -p {FAKE} registry"),
                "docker login -u me -p [redacted] registry",
            ),
            (
                format!("mysql -u root -p{FAKE} db"),
                "mysql -u root -p[redacted] db",
            ),
            (
                format!("curl -u me:{FAKE} https://x"),
                "curl -u me:[redacted] https://x",
            ),
            (
                format!("curl --user=me:{FAKE} x"),
                "curl --user=me:[redacted] x",
            ),
        ]);
        keeps(&[
            "ssh -p 22 host",
            "docker run -p 8080:80 image",
            "mysql -p database",
            "docker run -u 1000:1000 image",
            "app --password-file /run/secrets/db --token-limit 5",
            "app --no-password run",
            "app --password --verbose",
            "curl -u me https://x",
            "grep -pattern file",
        ]);
    }

    #[test]
    fn glued_quoted_and_unusual_spacing_forms() {
        masks(&[
            (
                format!("docker login -u me -p{FAKE} registry"),
                "docker login -u me -p[redacted] registry",
            ),
            (
                format!("podman login -p{FAKE}"),
                "podman login -p[redacted]",
            ),
            (
                format!("curl -ume:{FAKE} https://x"),
                "curl -ume:[redacted] https://x",
            ),
            (
                format!("Authorization: {FAKE}"),
                "Authorization: [redacted]",
            ),
            (
                "-H \"Authorization: abc12345\"".to_owned(),
                "-H \"Authorization: [redacted]\"",
            ),
            (
                format!("app --password\t{FAKE}"),
                "app --password\t[redacted]",
            ),
            (
                format!("app --password\u{a0}{FAKE}"),
                "app --password\u{a0}[redacted]",
            ),
            (
                format!("API_KEY =\u{a0}{FAKE}"),
                "API_KEY =\u{a0}[redacted]",
            ),
            (
                "app --password \"FAKE\\\"TAIL0123\" next".to_owned(),
                "app --password \"[redacted]\" next",
            ),
            (
                "curl -u \"me:FAKE PART2\" https://x".to_owned(),
                "curl -u \"me:[redacted]\" https://x",
            ),
            (
                "sshpass -p'FAKE PART2' ssh host".to_owned(),
                "sshpass -p'[redacted]' ssh host",
            ),
            (
                format!("it's done: sshpass -p {FAKE} ssh"),
                "it's done: sshpass -p [redacted] ssh",
            ),
        ]);
        keeps(&[
            "Authorization: required for basic users",
            "docker run -p8080:80 image",
            "tar -uf archive.tar file",
            "app --password\nnext line",
        ]);
    }

    /// Every pass must stay linear: a transcript line of up to 1 MiB is masked on the watcher
    /// thread, and a quadratic case froze it for over a minute.
    #[test]
    fn long_repetitive_lines_stay_fast() {
        // A wall clock bound alone failed on slow CI runners in debug builds, so the test compares
        // growth instead: eight times the input may take at most about eight times as long
        // (24x leaves room for noise; quadratic growth would be 64x). The absolute bound only
        // catches a pass that is far too slow everywhere.
        let limit = std::time::Duration::from_secs(10);
        let time = |text: &str| {
            let started = std::time::Instant::now();
            let _ = redact(&strip_controls(text));
            started.elapsed()
        };
        for unit in [
            "key:",
            "auth:",
            "cookie:",
            "\"key\":x",
            "token:",
            "--password ",
            "Authorization: ",
            "Bearer ",
            "a://",
            "-p",
            "\"",
            "\u{1b}[",
        ] {
            let small = time(&unit.repeat(125_000 / unit.len()));
            let large = time(&unit.repeat(1_000_000 / unit.len()));
            assert!(large < limit, "{unit:?} took {large:?}");
            // Below a few milliseconds timer noise dominates the ratio.
            let floor = std::time::Duration::from_millis(5);
            assert!(
                large <= small.max(floor) * 24,
                "{unit:?} grew from {small:?} to {large:?} for eight times the input"
            );
        }
    }

    #[test]
    fn authorization_schemes_and_secure_strings() {
        masks(&[
            (
                "Authorization: Basic dXNlcjpwYXNz".to_owned(),
                "Authorization: Basic [redacted]",
            ),
            (
                "-H \"Authorization: token FAKEVAL1\"".to_owned(),
                "-H \"Authorization: token [redacted]\"",
            ),
            (
                format!("{{\"Authorization\": \"Bearer {FAKE}\"}}"),
                "{\"Authorization\": \"Bearer [redacted]\"}",
            ),
            (
                format!("$p = ConvertTo-SecureString \"{FAKE}\" -AsPlainText -Force"),
                "$p = ConvertTo-SecureString \"[redacted]\" -AsPlainText -Force",
            ),
            (
                format!("ConvertTo-SecureString -String '{FAKE}' -AsPlainText"),
                "ConvertTo-SecureString -String '[redacted]' -AsPlainText",
            ),
        ]);
        keeps(&[
            "Authorization: required for basic users",
            "Authorization: Basic auth",
            "a basic understanding of tokens",
            "ConvertTo-SecureString -AsPlainText needs a value",
        ]);
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
            "SG.",
            "dop_v1_",
            "shpat_",
            "ya29.",
        ] {
            let input = format!("use {prefix}{FAKE} now");
            assert_eq!(redact(&input), "use [redacted] now", "{prefix}");
        }
        keeps(&[
            "pip install scikit-learn sk-learn",
            "a task-sk-x",
            "AKIA",
            "SG.short and ya29.x",
        ]);
    }

    #[test]
    fn headers_urls_and_yaml() {
        masks(&[
            (
                format!("curl -H \"Authorization: Bearer {FAKE}\" x"),
                "curl -H \"Authorization: Bearer [redacted]\" x",
            ),
            (
                format!("-H 'x-api-key: {FAKE}'"),
                "-H 'x-api-key: [redacted]'",
            ),
            (
                format!("password: {FAKE}\nuser: me"),
                "password: [redacted]\nuser: me",
            ),
            (
                format!("DATABASE_URL=postgres://app:{FAKE}@db:5432/x"),
                "DATABASE_URL=postgres://app:[redacted]@db:5432/x",
            ),
            (
                format!("git clone https://ghp_{FAKE}@github.com/a/b"),
                "git clone https://[redacted]@github.com/a/b",
            ),
            (
                format!("cmd --api=sk-{FAKE} /AKIA{FAKE}"),
                "cmd --api=[redacted] /[redacted]",
            ),
            (
                format!("token was Bearer {FAKE}"),
                "token was Bearer [redacted]",
            ),
        ]);
        keeps(&[
            "the bearer of bad news",
            "see https://example.com/path?q=1 and ssh://git@example.com",
            "Bearer tokens expire",
        ]);
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
    fn terminal_sequences_go_before_the_mask() {
        let colored = format!("\u{1b}[1;31mAPI_KEY\u{1b}[0m={FAKE}");
        assert_eq!(redact(&strip_controls(&colored)), "API_KEY=[redacted]");
        let linked = format!("\u{1b}]8;;https://x\u{7}TOKEN\u{1b}]8;;\u{1b}\\={FAKE}");
        assert_eq!(redact(&strip_controls(&linked)), "TOKEN=[redacted]");
        let c1 = format!("\u{9b}32mpassword\u{9b}0m: {FAKE}");
        assert_eq!(redact(&strip_controls(&c1)), "password: [redacted]");
        assert_eq!(
            strip_controls("a\u{1b}(Bb\u{7}\u{202e}c\u{200b}\r\nd\te\u{85}"),
            "abc\nd\te"
        );
        // An unterminated OSC swallows the rest: nothing of it can reach the text.
        assert_eq!(strip_controls("x\u{1b}]0;title"), "x");
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
            "Authorization:",
            "Basic ",
            "--password ",
            "-p",
            "sshpass ",
            "-u ",
            "Cookie:",
            "ConvertTo-SecureString ",
            "\u{1b}[",
            "\u{1b}]",
            "\n",
        ];
        for a in parts {
            for b in parts {
                for c in parts {
                    let text = format!("{a}{b}{c}{a}{FAKE}{b}");
                    let _ = redact(&strip_controls(&text));
                    let _ = redact(&text);
                }
            }
        }
    }

    #[test]
    fn non_ascii_text_is_kept_intact() {
        let input = format!("naïve café TOKEN={FAKE} 中文");
        assert_eq!(redact(&input), "naïve café TOKEN=[redacted] 中文");
        let input = format!("sshpass -p {FAKE} ssh 中文主机");
        assert_eq!(redact(&input), "sshpass -p [redacted] ssh 中文主机");
    }
}
