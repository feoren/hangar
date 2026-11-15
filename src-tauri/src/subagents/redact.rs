//! Masks what looks like a secret in transcript text before it leaves the sub-agent source.
//!
//! A best-effort filter, not a guarantee: it catches the common shapes an agent prints when it
//! reads a `.env` file, a config or a command line (`API_KEY=...`, `"token": "..."`,
//! `password: ...`, `--password ...`, `sshpass -p ...`, `curl -u user:...`,
//! `Authorization: Bearer|Basic|token ...`, `https://user:pass@host`,
//! `ConvertTo-SecureString "..." -AsPlainText`, `?X-Amz-Signature=...`, `&sig=...`,
//! `.netrc` entries, `<password>...</password>`, Slack and Discord webhook URLs), well-known
//! token prefixes and PEM private keys. An unquoted `password: ...` runs to the end of its line.
//! Over-masking is preferred to a leak. Callers strip terminal sequences first
//! ([`strip_controls`]), so a color code cannot split a name from its value.

pub const REDACTED: &str = "[redacted]";

/// Names whose value is masked when they end in one of these (case-insensitive): `API_KEY`,
/// `GITHUB_TOKEN`, `client_secret`, `password`, `--passphrase`, `aws_credentials`, and the
/// signature of a presigned URL (`X-Amz-Signature`, `X-Goog-Signature`); its
/// `X-Amz-Security-Token` already ends in `token`.
const SECRET_SUFFIXES: &[&str] = &[
    "key",
    "signature",
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
/// (`DB_PASS`, npm `_auth`, `Set-Cookie`, the `sig` of an Azure SAS URL), so `compass`, `oauth2`
/// or `config` stay.
const SECRET_WORDS: &[&str] = &["pass", "auth", "cookie", "sig"];

/// Bare names that are far more often a heading in prose than a secret: an unquoted
/// `name: value` masks only a value that looks like one (long, or holding `=`).
const LOOSE_NAMES: &[&str] = &["key", "auth", "cookie", "sig", "signature"];
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
/// How far past the first `/` a URL password may reach its `@`.
const MAX_URL_PASSWORD: usize = 128;

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
    let text = redact_webhooks(&text);
    let text = redact_xml(&text);
    let text = redact_netrc(&text);
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

/// Last parts of a name that make it the description of a secret rather than the secret
/// (`--password-file`, `TOKEN_COUNT`, `api_key_path`), when the secret word is not last.
const NOT_SECRET_TAILS: &[&str] = &[
    "file",
    "files",
    "path",
    "dir",
    "folder",
    "limit",
    "count",
    "len",
    "length",
    "size",
    "type",
    "name",
    "names",
    "id",
    "ids",
    "env",
    "var",
    "field",
    "header",
    "prefix",
    "suffix",
    "url",
    "uri",
    "endpoint",
    "expiry",
    "expires",
    "expiration",
    "ttl",
    "timeout",
    "format",
    "mode",
    "min",
    "max",
    "required",
    "policy",
    "helper",
    "store",
    "cache",
    "manager",
    "enabled",
    "disabled",
    "index",
    "usage",
    "used",
    "budget",
    "total",
    "bits",
    "algorithm",
    "version",
    "at",
    "date",
    "time",
    "ts",
    "status",
    "kind",
    "scope",
    "scopes",
];

/// Words in a name that make even a number its value worth masking (a numeric password or PIN).
const PASSWORD_WORDS: &[&str] = &["pass", "pwd", "pin"];

/// A secret-looking name, compared in lower case and without the dashes of a command line flag:
/// it ends in a secret word, or a secret word is followed by a qualifier (`API_KEY_2`,
/// `SECRET_KEY_BASE`, `GITHUB_TOKEN_OLD`) that does not describe the secret
/// ([`NOT_SECRET_TAILS`]).
fn is_secret_name(lower: &str) -> bool {
    let bare = lower.trim_start_matches('-');
    if ends_in_secret(bare) {
        return true;
    }
    let last = bare.rsplit(['_', '-']).next().unwrap_or(bare);
    if NOT_SECRET_TAILS.contains(&last) {
        return false;
    }
    bare.char_indices()
        .filter(|(_, c)| matches!(c, '_' | '-'))
        .any(|(at, _)| ends_in_secret(&bare[..at]))
}

fn ends_in_secret(bare: &str) -> bool {
    SECRET_SUFFIXES.iter().any(|s| bare.ends_with(s))
        || SECRET_WORDS.iter().any(|word| {
            bare == *word
                || bare
                    .strip_suffix(word)
                    .is_some_and(|head| head.ends_with('_') || head.ends_with('-'))
        })
}

/// `text` holds `pattern` at `at`, ASCII case ignored.
fn holds_at(text: &str, at: usize, pattern: &str) -> bool {
    text.as_bytes()
        .get(at..at + pattern.len())
        .is_some_and(|found| found.eq_ignore_ascii_case(pattern.as_bytes()))
}

/// An unquoted value that is no secret: a number (unless the name is a password's), a boolean,
/// or code that calls something (`load_key(path)`).
fn harmless_value(text: &str, start: usize, end: usize, bare_name: &str) -> bool {
    let value = &text[start..end];
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "null" | "none" | "nil" | "on" | "off"
    ) {
        return true;
    }
    let number = !value.is_empty()
        && value
            .trim_start_matches('-')
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
        && value.bytes().any(|b| b.is_ascii_digit());
    if number && !PASSWORD_WORDS.iter().any(|word| bare_name.contains(word)) {
        return true;
    }
    let call = value
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':'))
        .count();
    call > 0
        && value.as_bytes().get(call) == Some(&b'(')
        && value.as_bytes()[0].is_ascii_alphabetic()
}

/// The unquoted value starting at `start`: up to whitespace, a separator or the quote that closes
/// a surrounding argument (`-H 'x-api-key: ...'`). A comma inside the value belongs to it
/// (`p@ss,word`); trailing commas do not (`f(password=x, user=y)`).
fn unquoted_end(text: &str, start: usize) -> usize {
    let end = text[start..]
        .find(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '}' | ')' | '"' | '\''))
        .map_or(text.len(), |e| start + e);
    start + text[start..end].trim_end_matches(',').len()
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
        let mut name = text[start..i].to_ascii_lowercase();
        // `%22name%22`: the name of URL-encoded JSON starts after the `22` of its quote.
        let encoded_name = start > 0 && bytes[start - 1] == b'%' && name.starts_with("22");
        if encoded_name {
            name.drain(..2);
        }
        if !is_secret_name(&name) {
            continue;
        }
        // `"name"`: the same quote on both sides (`'x-api-key: ...'` is a quoted header, not a
        // quoted name). Also `\"name\"` (JSON inside a JSON string) and `%22name%22`.
        let open = start.checked_sub(1).map(|at| bytes[at]);
        let name_end = match open {
            Some(quote @ (b'"' | b'\'')) if bytes.get(i) == Some(&quote) => Some(i + 1),
            Some(quote @ (b'"' | b'\''))
                if bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&quote) =>
            {
                Some(i + 2)
            }
            // The opening `%22` may have been cut off; an encoded separator then tells.
            _ if holds_at(text, i, "%22")
                && (encoded_name
                    || holds_at(text, i + 3, "%3a")
                    || holds_at(text, i + 3, "%3d")) =>
            {
                Some(i + 3)
            }
            _ => None,
        };
        let quoted_name = name_end.is_some();
        let flag = name.starts_with('-') && !name.starts_with("--no-");
        let mut j = skip_spaces(text, name_end.unwrap_or(i));
        let Some(&first) = bytes.get(j) else {
            break;
        };
        let spaced = j > i;
        // The separator and its length: ASCII, fullwidth (`：`, `＝`) or URL-encoded.
        let (separator, separator_len) = match first {
            b'=' | b':' => (first, 1),
            _ if text[j..].starts_with('\u{ff1a}') => (b':', '\u{ff1a}'.len_utf8()),
            _ if text[j..].starts_with('\u{ff1d}') => (b'=', '\u{ff1d}'.len_utf8()),
            _ if holds_at(text, j, "%3a") => (b':', 3),
            _ if holds_at(text, j, "%3d") => (b'=', 3),
            _ => (first, 0),
        };
        let encoded = separator_len == 3 && first == b'%';
        if separator_len > 0 {
            j = skip_spaces(text, j + separator_len);
        } else if !(flag && spaced && separator != b'-' && separator != b'\n') {
            // `--password value`: a flag takes the next word; `--password --other` has none.
            continue;
        }
        let found = if encoded && holds_at(text, j, "%22") {
            // URL-encoded JSON: the value runs to the next encoded quote. Scanned by hand, so
            // the search ends with the value and the whole pass stays linear.
            let from = j + 3;
            let mut end = from;
            while end < bytes.len()
                && !holds_at(text, end, "%22")
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b';' | b'&' | b'}' | b')' | b'"' | b'\'')
            {
                end += 1;
            }
            Some((from, end))
        } else if let (Some(b'\\'), Some(&quote @ (b'"' | b'\''))) =
            (bytes.get(j), bytes.get(j + 1))
        {
            // `\"value\"`: up to the escaped closing quote, a single backslash before it. A
            // quote inside the value has three (`\\\"`), an escaped backslash two.
            let from = j + 2;
            let mut at = from;
            while at < bytes.len() {
                if bytes[at] != b'\\' {
                    at += 1;
                    continue;
                }
                let run = bytes[at..].iter().take_while(|b| **b == b'\\').count();
                if run == 1 && bytes.get(at + 1) == Some(&quote) {
                    break;
                }
                at += run;
            }
            Some((from, at.min(bytes.len())))
        } else {
            value_at(text, j)
        };
        let Some((value_start, mut value_end)) = found else {
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
        if !quoted_value && harmless_value(text, value_start, value_end, bare) {
            i = value_end.max(i);
            continue;
        }
        if separator == b':' && !quoted_value {
            if let Some(block) = yaml_block(text, start, value_start) {
                if let Some((from, to)) = block {
                    ranges.push((from, to));
                }
                i = block.map_or(value_end, |(_, to)| to).max(i);
                continue;
            }
        }
        if separator == b':' && !quoted_name && LOOSE_NAMES.contains(&bare) && !quoted_value {
            let value = &text[value_start..value_end];
            let token_len = value.bytes().take_while(|b| is_token_byte(*b)).count();
            if token_len < MIN_LOOSE_VALUE && !value.contains('=') {
                i = value_end.max(i);
                continue;
            }
        }
        let to_line_end = bare.ends_with("cookie")
            || (separator == b':' && !quoted_name && !LOOSE_NAMES.contains(&bare));
        if to_line_end && !quoted_value {
            // `Cookie: a=1; b=2` holds several values, `password: two words` one with spaces:
            // the rest of the line, or up to the quote that closes a surrounding argument.
            value_end = text[value_start..]
                .find(['\n', '"', '\''])
                .map_or(text.len(), |e| value_start + e);
            value_end = value_start + text[value_start..value_end].trim_end().len();
        }
        if value_end > value_start {
            ranges.push((value_start, value_end));
        }
        i = value_end.max(i);
    }
    mask_ranges(text, &ranges)
}

/// A YAML block scalar (`password: |` or `>`, with chomping and indentation indicators): the
/// lines below the name that are indented deeper than its own line. `marker` is where the value
/// starts on the name's line. `Some(None)`: a block indicator without lines below it.
fn yaml_block(text: &str, name_start: usize, marker: usize) -> Option<Option<(usize, usize)>> {
    // The indicator is a few characters at most; looking no further keeps the pass linear.
    let near = &text.as_bytes()[marker..text.len().min(marker + 16)];
    if !matches!(near.first(), Some(b'|' | b'>')) {
        return None;
    }
    let line_end = match near.iter().position(|b| *b == b'\n') {
        Some(at) => marker + at,
        None if marker + near.len() == text.len() => text.len(),
        None => return None,
    };
    let indicator = text[marker..line_end].trim_end();
    let mut chars = indicator.chars();
    if !matches!(chars.next(), Some('|' | '>'))
        || !chars.all(|c| matches!(c, '-' | '+' | '1'..='9'))
    {
        return None;
    }
    let line_start = text[..name_start].rfind('\n').map_or(0, |at| at + 1);
    let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    let own = indent(&text[line_start..name_start]);
    let mut first = None;
    let mut last = None;
    let mut at = line_end;
    while at < text.len() {
        let start = at + 1;
        let end = text[start..].find('\n').map_or(text.len(), |e| start + e);
        let line = &text[start..end];
        if !line.trim().is_empty() {
            if indent(line) <= own {
                break;
            }
            first.get_or_insert(start + indent(line));
            last = Some(start + line.trim_end().len());
        }
        at = end;
    }
    Some(first.zip(last))
}

/// Webhook URLs whose path is the credential: anyone who has it can post to the channel.
const WEBHOOKS: &[&str] = &[
    "hooks.slack.com/services/",
    "hooks.slack.com/workflows/",
    "hooks.slack.com/triggers/",
    "discord.com/api/webhooks/",
    "discordapp.com/api/webhooks/",
];

/// Slack and Discord webhook URLs: the path after the fixed part.
fn redact_webhooks(text: &str) -> String {
    // ASCII lower case keeps every byte offset.
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    for pattern in WEBHOOKS {
        let mut from = 0;
        while let Some(found) = lower[from..].find(pattern) {
            let start = from + found + pattern.len();
            let len = bytes[start..]
                .iter()
                .take_while(|b| is_token_byte(**b))
                .count();
            if len > 0 {
                ranges.push((start, start + len));
            }
            from = start + len;
        }
    }
    ranges.sort_unstable();
    mask_ranges(text, &ranges)
}

/// `<password>value</password>`, `<apiKey>...</apiKey>`, `<ns:Secret attr="x">...</ns:Secret>`:
/// the text of an element with a secret-looking name, when a closing tag follows it. Loose names
/// stay (a plist names every entry in `<key>`).
fn redact_xml(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find('<') {
        let open = from + found;
        from = open + 1;
        let name_end = from
            + bytes[from..]
                .iter()
                .take_while(|b| is_name_byte(**b) || **b == b':')
                .count();
        if name_end == from || !matches!(bytes.get(name_end), Some(b'>' | b' ' | b'\t')) {
            continue;
        }
        let name = text[from..name_end].to_ascii_lowercase();
        let local = name.rsplit(':').next().unwrap_or(&name);
        if LOOSE_NAMES.contains(&local) || !is_secret_name(local) {
            continue;
        }
        // The rest of the start tag on the same line; a self-closing tag holds nothing.
        let Some(close) = text[name_end..]
            .find(['>', '<', '\n'])
            .map(|e| name_end + e)
        else {
            continue;
        };
        if bytes[close] != b'>' || bytes[close - 1] == b'/' {
            continue;
        }
        let value_start = close + 1;
        let Some(value_end) = text[value_start..].find('<').map(|e| value_start + e) else {
            continue;
        };
        if !text[value_end..].starts_with("</") {
            continue;
        }
        if !text[value_start..value_end].trim().is_empty() {
            ranges.push((value_start, value_end));
        }
        from = value_end;
    }
    mask_ranges(text, &ranges)
}

/// `.netrc` entries: `machine <host> login <user> password <value>` (also `default` and
/// `account`), on one line or spread over the lines that follow `machine`. A line counts only
/// when all its words are such keyword-value pairs, and a line without `machine` or `default`
/// only right after one, so `password reset` in prose stays.
fn redact_netrc(text: &str) -> String {
    const KEYWORDS: &[&str] = &["machine", "login", "password", "account", "port"];
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut in_entry = false;
    for line in text.split_inclusive('\n') {
        let list = words(line, offset);
        offset += line.len();
        if list.is_empty() {
            continue;
        }
        let word = |&(start, end): &(usize, usize)| &text[start..end];
        // `default` stands alone; every other keyword takes a value.
        let pairs: Vec<&[(usize, usize)]> = list
            .split(|w| word(w) == "default")
            .flat_map(|run| run.chunks(2))
            .collect();
        let starts = list
            .iter()
            .any(|w| matches!(word(w), "machine" | "default"));
        let netrc = pairs.iter().all(|pair| {
            pair.len() == 2
                && pair
                    .first()
                    .is_some_and(|key| KEYWORDS.contains(&word(key)))
        });
        in_entry = netrc && (starts || in_entry);
        if !in_entry {
            continue;
        }
        for pair in pairs {
            if let [key, value] = pair {
                if matches!(word(key), "password" | "account") {
                    ranges.push(unquote(text, *value));
                }
            }
        }
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
        let mut authority_len = authority_len;
        // `https://user:pa/ss@host`: a password with a slash. The authority then runs to the `@`
        // that follows within [`MAX_URL_PASSWORD`] bytes, when the part after the colon is no
        // port. The bound keeps the pass linear over a line of `a://a://...`.
        if !after[..authority_len].contains('@') {
            if let Some(colon) = after[..authority_len].find(':') {
                let port = after[colon + 1..authority_len]
                    .bytes()
                    .all(|b| b.is_ascii_digit());
                let window = after
                    .char_indices()
                    .take_while(|(at, _)| *at <= authority_len + MAX_URL_PASSWORD)
                    .find(|(_, c)| {
                        c.is_whitespace() || matches!(c, '@' | '"' | '\'' | '<' | '>' | '`')
                    });
                if let (false, Some((at, '@'))) = (port, window) {
                    let host_end = after[at..]
                        .find(|c: char| {
                            c.is_whitespace()
                                || matches!(c, '/' | '?' | '#' | '"' | '\'' | '<' | '>' | '`')
                        })
                        .map_or(after.len(), |e| at + e);
                    authority_len = host_end;
                }
            }
        }
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
        // Switches (`-AsPlainText`, `-Force`) may come first, the value is named by `-String`
        // or comes as the first word that is no switch.
        at = skip_spaces(at);
        let mut value = None;
        for round in 0..4 {
            if lower.as_bytes().get(at) != Some(&b'-') {
                // A word after a switch is the value only if quoted or not a plain word
                // (`-AsPlainText needs a value` is prose).
                let word = text[at..]
                    .split(|c: char| c.is_whitespace())
                    .next()
                    .unwrap_or_default();
                let plain = !word.is_empty() && word.chars().all(char::is_alphabetic);
                if round == 0 || !plain {
                    value = Some(at);
                }
                break;
            }
            let word_end = lower[at..]
                .find(|c: char| c.is_whitespace() || c == ':')
                .map_or(lower.len(), |e| at + e);
            let named = &lower[at..word_end] == "-string";
            at = skip_spaces(word_end + usize::from(lower.as_bytes().get(word_end) == Some(&b':')));
            if named {
                value = Some(at);
                break;
            }
        }
        let Some(at) = value else {
            continue;
        };
        if matches!(
            lower.as_bytes().get(at),
            None | Some(b'|' | b';' | b'\n' | b')')
        ) {
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
            "\\\"token\\\": \\\"",
            "%22token%22%3A%22",
            "token%3D",
            "password: |\n ",
            "https://u:p/",
            "ConvertTo-SecureString -AsPlainText ",
            "API_KEY_2_",
            "password=f(",
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
    fn signatures_of_presigned_urls() {
        masks(&[
            (
                format!(
                    "curl 'https://bucket.s3.amazonaws.com/a.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIDEXAMPLE%2F20261111%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20261111T000000Z&X-Amz-Expires=3600&X-Amz-Security-Token={FAKE}&X-Amz-SignedHeaders=host&X-Amz-Signature={FAKE}'"
                ),
                "curl 'https://bucket.s3.amazonaws.com/a.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=[redacted]&X-Amz-Date=20261111T000000Z&X-Amz-Expires=3600&X-Amz-Security-Token=[redacted]&X-Amz-SignedHeaders=host&X-Amz-Signature=[redacted]'",
            ),
            (
                format!(
                    "https://acct.blob.core.windows.net/c/b.txt?sv=2022-11-02&ss=b&srt=o&sp=r&se=2026-11-12T00:00:00Z&spr=https&sig={FAKE}%3D next"
                ),
                "https://acct.blob.core.windows.net/c/b.txt?sv=2022-11-02&ss=b&srt=o&sp=r&se=2026-11-12T00:00:00Z&spr=https&sig=[redacted] next",
            ),
            (
                format!("https://storage.googleapis.com/b/o?X-Goog-Signature={FAKE}"),
                "https://storage.googleapis.com/b/o?X-Goog-Signature=[redacted]",
            ),
        ]);
        keeps(&[
            "the signature: looks fine",
            "check the function signature first",
            "sig: ok",
            "config=release",
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
    fn netrc_entries() {
        masks(&[
            (
                format!("machine example.invalid login me password {FAKE}"),
                "machine example.invalid login me password [redacted]",
            ),
            (
                format!("default login me password {FAKE} account FAKEACCOUNT"),
                "default login me password [redacted] account [redacted]",
            ),
            (
                format!("machine example.invalid\n  login me\n  password {FAKE}\n"),
                "machine example.invalid\n  login me\n  password [redacted]\n",
            ),
        ]);
    }

    #[test]
    fn xml_elements() {
        masks(&[
            (
                format!("<password>{FAKE}</password>"),
                "<password>[redacted]</password>",
            ),
            (
                format!("<Settings><apiKey>{FAKE}</apiKey><n>1</n></Settings>"),
                "<Settings><apiKey>[redacted]</apiKey><n>1</n></Settings>",
            ),
            (
                format!("<db:Password type=\"plain\">{FAKE}</db:Password>"),
                "<db:Password type=\"plain\">[redacted]</db:Password>",
            ),
        ]);
        // A plist names its keys in `<key>`; `<password/>` holds nothing.
        keeps(&[
            "<key>CFBundleName</key><string>Hangar</string>",
            "<password/>",
            "<password></password>",
        ]);
    }

    #[test]
    fn unquoted_values_with_spaces_run_to_the_line_end() {
        masks(&[
            (
                "password: FAKE words with spaces\nuser: me".to_owned(),
                "password: [redacted]\nuser: me",
            ),
            (
                "db_secret: FAKE two parts".to_owned(),
                "db_secret: [redacted]",
            ),
            (
                format!("-H 'x-api-key: {FAKE} x' -d y"),
                "-H 'x-api-key: [redacted]' -d y",
            ),
        ]);
    }

    #[test]
    fn webhook_urls() {
        masks(&[
            (
                "post to https://hooks.slack.com/services/T0FAKE/B0FAKE/FAKEVALUE now".to_owned(),
                "post to https://hooks.slack.com/services/[redacted] now",
            ),
            (
                "url=\"https://discord.com/api/webhooks/0000/FAKEVALUE-x\"".to_owned(),
                "url=\"https://discord.com/api/webhooks/[redacted]\"",
            ),
            (
                "https://discordapp.com/api/webhooks/0000/FAKEVALUE".to_owned(),
                "https://discordapp.com/api/webhooks/[redacted]",
            ),
        ]);
        keeps(&["see https://hooks.slack.com/ and https://discord.com/api/webhooks/"]);
    }

    /// Everyday sentences about passwords and keys are no secrets.
    #[test]
    fn prose_about_secrets_stays() {
        keeps(&[
            "Forgot your password? Reset it in the settings.",
            "The password is wrong, try again.",
            "password is wrong here",
            "password reset",
            "machine learning is fun",
            "Use a passphrase manager and rotate the token often.",
            "compass: north, then the machine room",
            "The machine has no login password set.",
            "Read the <token> docs and the key: value section.",
        ]);
    }

    /// JSON inside a JSON string (`{\"password\": \"...\"}`), as an agent's tool input shows it.
    #[test]
    fn escaped_json_names_and_values() {
        masks(&[
            (
                format!(r#"{{\"password\": \"{FAKE}\"}}"#),
                r#"{\"password\": \"[redacted]\"}"#,
            ),
            (
                format!(r#"{{\"api_key\":\"{FAKE}\",\"n\":1}}"#),
                r#"{\"api_key\":\"[redacted]\",\"n\":1}"#,
            ),
            (
                format!(r#"{{\"token\": \"FAKE\\\"QUOTE{FAKE}\"}}"#),
                r#"{\"token\": \"[redacted]\"}"#,
            ),
        ]);
        keeps(&[r#"{\"token\": 12, \"key\": {}}"#]);
    }

    #[test]
    fn names_with_a_qualifier_after_the_secret_word() {
        masks(&[
            (format!("API_KEY_2={FAKE}"), "API_KEY_2=[redacted]"),
            (
                format!("SECRET_KEY_BASE={FAKE}"),
                "SECRET_KEY_BASE=[redacted]",
            ),
            (
                format!("GITHUB_TOKEN_OLD={FAKE}"),
                "GITHUB_TOKEN_OLD=[redacted]",
            ),
            (
                format!("db-password-prod: {FAKE}"),
                "db-password-prod: [redacted]",
            ),
        ]);
        keeps(&[
            "TOKEN_COUNT=5",
            "api_key_path=/etc/app/key.pem",
            "app --password-file /run/secrets/db",
            "GITHUB_TOKEN_NAME=deploy",
            "auth_token_refreshed_at=2024-01-01",
            "API_KEY_CREATED_DATE=2024-01-01",
            "TOKEN_SCOPES=repo,workflow",
        ]);
    }

    #[test]
    fn other_separators_and_encodings() {
        masks(&[
            (
                format!("PASSWORD\u{ff1a}{FAKE}"),
                "PASSWORD\u{ff1a}[redacted]",
            ),
            (
                format!("API_KEY\u{ff1d}{FAKE}"),
                "API_KEY\u{ff1d}[redacted]",
            ),
            (
                format!("?password%3D{FAKE}&x=1"),
                "?password%3D[redacted]&x=1",
            ),
            (
                format!("q=%7B%22token%22%3A%22{FAKE}%22%7D next"),
                "q=%7B%22token%22%3A%22[redacted]%22%7D next",
            ),
            (format!("token%22%3A%22{FAKE}"), "token%22%3A%22[redacted]"),
            (
                "db_password=p@ss,word9 next".to_owned(),
                "db_password=[redacted] next",
            ),
            (
                format!("connect(password={FAKE}, user=me)"),
                "connect(password=[redacted], user=me)",
            ),
            // A numeric password is still one.
            ("DB_PASSWORD=12345678".to_owned(), "DB_PASSWORD=[redacted]"),
        ]);
    }

    #[test]
    fn yaml_block_scalars() {
        masks(&[
            (
                format!("password: |\n  {FAKE}\n  second line\nuser: me"),
                "password: |\n  [redacted]\nuser: me",
            ),
            (
                format!("db:\n  secret: >-\n    {FAKE}\n\n    more\n  host: x"),
                "db:\n  secret: >-\n    [redacted]\n  host: x",
            ),
        ]);
        // An empty block holds nothing.
        keeps(&["password: |\nuser: me"]);
    }

    #[test]
    fn secure_strings_and_url_passwords_in_any_order() {
        masks(&[
            (
                format!("ConvertTo-SecureString -AsPlainText -Force -String '{FAKE}'"),
                "ConvertTo-SecureString -AsPlainText -Force -String '[redacted]'",
            ),
            (
                format!("ConvertTo-SecureString -AsPlainText -Force \"{FAKE}\""),
                "ConvertTo-SecureString -AsPlainText -Force \"[redacted]\"",
            ),
            (
                format!("ConvertTo-SecureString -String:{FAKE} -AsPlainText"),
                "ConvertTo-SecureString -String:[redacted] -AsPlainText",
            ),
            (
                "git clone https://user:pa/ss@host.example/x".to_owned(),
                "git clone https://user:[redacted]@host.example/x",
            ),
        ]);
        keeps(&[
            "ConvertTo-SecureString -AsPlainText -Force | Out-Null",
            "http://localhost:3000/users/@me",
            "https://example.com/a?mail=a@b.example",
        ]);
    }

    /// Numbers, booleans and code are no secrets, whatever the name.
    #[test]
    fn harmless_values_stay() {
        keeps(&[
            "TOKEN=5",
            "primary_key=True",
            "api_key = None",
            "let api_key = load_key(path);",
            "secret = os.environ.get(name)",
            "Input token: 123 and the rest of the line",
            "\"use_auth\": false",
        ]);
    }

    #[test]
    fn non_ascii_text_is_kept_intact() {
        let input = format!("naïve café TOKEN={FAKE} 中文");
        assert_eq!(redact(&input), "naïve café TOKEN=[redacted] 中文");
        let input = format!("sshpass -p {FAKE} ssh 中文主机");
        assert_eq!(redact(&input), "sshpass -p [redacted] ssh 中文主机");
    }
}
