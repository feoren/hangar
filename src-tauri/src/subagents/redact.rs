//! Masks what looks like a secret in transcript text before it leaves the sub-agent source.
//!
//! A best-effort filter, not a guarantee: it catches the common shapes an agent prints when it
//! reads a `.env` file, a config or a command line (`API_KEY=...`, `"token": "..."`,
//! `password: ...`, `--password ...`, `sshpass -p ...`, `curl -u user:...`,
//! `Authorization: Bearer|Basic|token ...`, `https://user:pass@host`,
//! `ConvertTo-SecureString "..." -AsPlainText`, `?X-Amz-Signature=...`, `&sig=...`,
//! `.netrc` entries, `<password>...</password>`, `<add key="ApiKey" value="..."/>`, Slack,
//! Discord and Teams webhook URLs), the other separators of code (`=>`, `:=`, `<-`,
//! `os.environ['NAME'] = ...`, `SetEnvironmentVariable('NAME', ...)`), variables set without `=`
//! (`setx`, fish `set -x`, Dockerfile `ENV`), a line of just a secret name and a token, a YAML
//! value on the line below its name, `Your API key is <token>`, well-known token prefixes, PEM
//! and SSH2 private keys and PuTTY `.ppk` private lines, `.pgpass` lines, XML elements named by a
//! `key` or `name` attribute (`<string name="api_key">...</string>`), config commands that set a
//! secret key (`git config github.token ...`, `npm config set ...:_authToken ...`), Markdown table
//! rows (`| DB_PASSWORD | ... |`), fallback literals (`os.getenv("X", "...")`, `|| '...'`) and
//! the password flags of common tools (`redis-cli -a`, `ldapsearch -w`, `smbclient -U u%p`,
//! `net use`, `openssl -k`, `twine -p`). An unquoted `password: ...` runs to the end of its line,
//! the value of a `.env` line to the next whitespace.
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
    // Slack client, cookie and refresh tokens.
    ("xoxc-", 10),
    ("xoxd-", 10),
    ("xoxe-", 10),
    ("xoxe.", 10),
    ("xapp-", 10),
    // AWS access key ids, long-term and temporary; see [`AWS_KEY_IDS`].
    ("AKIA", 16),
    ("ASIA", 16),
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
    // PyPI upload tokens (a macaroon), Stripe and Svix webhook secrets, age identities, Grafana
    // service account tokens, Heroku API keys.
    ("pypi-AgE", 50),
    ("whsec_", 24),
    ("AGE-SECRET-KEY-1", 50),
    ("glsa_", 32),
    ("HRKU-", 24),
];

/// A Telegram bot token: 8 to 10 digits of the bot id, `:AA`, then this many characters.
const MIN_TELEGRAM_TAIL: usize = 30;

/// AWS key ids are 16 upper case letters or digits after the prefix, so that names such as
/// `ASIA_PACIFIC_REGION` stay.
const AWS_KEY_IDS: &[&str] = &["AKIA", "ASIA"];

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

const PEM_BEGIN: &str = "BEGIN ";
const PEM_END: &str = "END ";
/// The shortest run of dashes around a key block's label (SSH2 public and private keys use four).
const PEM_DASHES: &str = "----";
/// How far a key block's label may reach its closing dashes.
const MAX_PEM_LABEL: usize = 96;

pub fn redact(text: &str) -> String {
    let text = redact_pem(text);
    let text = redact_ppk(&text);
    let text = redact_pgpass(&text);
    let text = redact_url_credentials(&text);
    let text = redact_authorization(&text);
    let text = redact_bearer(&text);
    let text = redact_secure_string(&text);
    let text = redact_tool_flags(&text);
    let text = redact_webhooks(&text);
    let text = redact_xml(&text);
    let text = redact_xml_attributes(&text);
    let text = redact_netrc(&text);
    let text = redact_table_cells(&text);
    let text = redact_fallbacks(&text);
    let text = redact_word_lines(&text);
    let text = redact_assignments(&text);
    let text = redact_sentences(&text);
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
/// (or the end of the text, when cut). The dashes may be four or more and spaced off the words
/// (`---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----`).
fn redact_pem(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(begin) = rest.find(PEM_BEGIN) {
        let after = &rest[begin + PEM_BEGIN.len()..];
        let open = pem_dashes_before(rest, begin);
        // The label is short; looking no further keeps the pass linear.
        let window = &after[..floor_boundary(after, MAX_PEM_LABEL)];
        let label_end = window.find(PEM_DASHES).filter(|_| open.is_some());
        let label = label_end.map(|end| after[..end].trim());
        let is_key = label.is_some_and(|label| {
            !label.contains('\n') && (label.ends_with("KEY") || label.contains("PRIVATE KEY"))
        });
        let (Some(open), true) = (open, is_key) else {
            out.push_str(&rest[..begin + PEM_BEGIN.len()]);
            rest = after;
            continue;
        };
        out.push_str(&rest[..open]);
        out.push_str(REDACTED);
        rest = match pem_end(after) {
            Some(end) => {
                let tail = &after[end + PEM_END.len()..];
                let close = tail[..floor_boundary(tail, MAX_PEM_LABEL)].find(PEM_DASHES);
                close.map_or("", |close| {
                    let dashes = tail[close..].bytes().take_while(|b| *b == b'-').count();
                    &tail[close + dashes..]
                })
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// The first `END ` behind a run of dashes.
fn pem_end(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find(PEM_END) {
        let at = from + found;
        if pem_dashes_before(text, at).is_some() {
            return Some(at);
        }
        from = at + PEM_END.len();
    }
    None
}

/// Where the dashes before `BEGIN ` at `begin` start (at least four, spaces between allowed).
fn pem_dashes_before(text: &str, begin: usize) -> Option<usize> {
    let head = text[..begin].trim_end_matches(' ');
    let dashes = head.bytes().rev().take_while(|b| *b == b'-').count();
    (dashes >= PEM_DASHES.len()).then(|| head.len() - dashes)
}

/// The largest character boundary of `text` at or below `max`.
fn floor_boundary(text: &str, max: usize) -> usize {
    let mut at = max.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// PuTTY private key files (`.ppk`): the lines after `Private-Lines: <n>` hold the key.
fn redact_ppk(text: &str) -> String {
    const NAME: &str = "Private-Lines:";
    const MAX_LINES: usize = 1024;
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut pending = 0usize;
    let mut block: Option<(usize, usize)> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        if pending > 0 {
            let end = offset + body.len();
            block = Some(block.map_or((offset, end), |(start, _)| (start, end)));
            pending -= 1;
        } else if let Some(count) = body.strip_prefix(NAME) {
            ranges.extend(block.take());
            pending = count
                .trim()
                .parse::<usize>()
                .map_or(0, |n| n.min(MAX_LINES));
        }
        if pending == 0 {
            ranges.extend(block.take());
        }
        offset += line.len();
    }
    ranges.extend(block);
    mask_ranges(text, &ranges)
}

/// `.pgpass` lines, `host:port:database:user:password`: five fields, the port a number or `*`,
/// no whitespace before the password. `\:` belongs to its field.
fn redact_pgpass(text: &str) -> String {
    const FIELDS: usize = 5;
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let lead = body.len() - body.trim_start().len();
        let body = &body[lead..];
        let bytes = body.as_bytes();
        let mut colons = Vec::with_capacity(FIELDS);
        let mut at = 0;
        while at < bytes.len() && colons.len() < FIELDS {
            match bytes[at] {
                b'\\' => at += 2,
                b':' => {
                    colons.push(at);
                    at += 1;
                }
                _ => at += 1,
            }
        }
        let [host, port, database, user] = match colons.as_slice() {
            &[a, b, c, d] => [(0, a), (a + 1, b), (b + 1, c), (c + 1, d)],
            _ => continue,
        };
        let field = |(start, end): (usize, usize)| &body[start..end];
        let head = &body[..user.1];
        let port = field(port);
        let port_ok = port == "*" || (!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
        let filled = [host, database, user].iter().all(|f| !field(*f).is_empty());
        if !port_ok || !filled || head.contains(char::is_whitespace) {
            continue;
        }
        let from = user.1 + 1;
        let password = body[from..].trim_end();
        if !password.is_empty() {
            let start = line_start + lead + from;
            ranges.push((start, start + password.len()));
        }
    }
    mask_ranges(text, &ranges)
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
        // `os.environ['API_TOKEN'] = ...`, `ENV["API_TOKEN"] = ...`: a subscript.
        let name_end = name_end.map(|end| end + usize::from(bytes.get(end) == Some(&b']')));
        let quoted_name = name_end.is_some();
        let flag = name.starts_with('-') && !name.starts_with("--no-");
        let mut j = skip_spaces(text, name_end.unwrap_or(i));
        let Some(&first) = bytes.get(j) else {
            break;
        };
        let spaced = j > i;
        // The separator and its length: ASCII, fullwidth (`：`, `＝`) or URL-encoded.
        let (separator, separator_len) = match first {
            // `:=` (Go), `=>` (PHP, Ruby, Perl), `<-` (R).
            _ if [":=", "=>", "<-"].iter().any(|s| text[j..].starts_with(s)) => (b'=', 2),
            b'=' | b':' => (first, 1),
            // `SetEnvironmentVariable('NAME', 'value')`, `setenv("NAME", "value")`.
            b',' if quoted_name && setenv_call(text, start) => (b'=', 1),
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
        // Masked by an earlier pass (`-passin pass:[redacted] -in x`): the rest is no value.
        if text[value_start..].starts_with(REDACTED) {
            i = (value_start + REDACTED.len()).max(i);
            continue;
        }
        // A `.env` line (`NAME=value`, also after `export` or a list's `- `): the value runs to
        // the next whitespace, `&`, `;`, `)`, `}` and quotes included; a `;` or `,` at its end
        // ends a shell command or list instead.
        if separator == b'=' && separator_len <= 2 && !quoted_value && env_line_start(text, start) {
            let end = text[value_start..]
                .find(char::is_whitespace)
                .map_or(text.len(), |e| value_start + e);
            value_end = value_start + text[value_start..end].trim_end_matches([';', ',']).len();
        }
        // `<add key="Password" value="..."/>`, `<entry key="password">...</entry>`: the name of
        // the entry, whose value the attribute or element pass masks.
        if name == "key" && quoted_value {
            let after = value_end + 1;
            let rest = text
                .get(after..text.len().min(after + 16))
                .unwrap_or_default();
            let rest = rest.trim_start();
            // `key="theme">`: the entry's name; only a long value could be a key itself.
            let entry = &text[value_start..value_end];
            let names_entry =
                rest.starts_with('>') && (named_secret(entry) || entry.len() < MIN_LOOSE_VALUE);
            if rest.starts_with("value=") || names_entry {
                i = value_end.max(i);
                continue;
            }
        }
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
        if separator == b':' && !quoted_value && value_start == value_end {
            // `password:` with the value on the next, deeper indented line.
            if let Some((from, to)) = yaml_next_line(text, start, value_start) {
                let value = &text[from..to];
                let token_len = value.bytes().take_while(|b| is_token_byte(*b)).count();
                let loose = LOOSE_NAMES.contains(&bare) && token_len < MIN_LOOSE_VALUE;
                if !loose && !harmless_value(text, from, to, bare) {
                    ranges.push((from, to));
                }
                i = to.max(i);
                continue;
            }
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

/// Whether the name at `start` begins a `.env` style line: nothing before it on its line but
/// spaces, `export ` or a list's `- `. Looks back over those few bytes only, so the pass stays
/// linear.
fn env_line_start(text: &str, start: usize) -> bool {
    let head = text[..start].trim_end_matches([' ', '\t']);
    let head = head
        .strip_suffix("export")
        .or_else(|| head.strip_suffix('-'))
        .map_or(head, |rest| rest.trim_end_matches([' ', '\t']));
    head.is_empty() || head.ends_with('\n')
}

/// Whether the quoted name at `start` is the first argument of a call that sets an environment
/// variable (`SetEnvironmentVariable(`, `setenv(`, `Setenv(`).
fn setenv_call(text: &str, start: usize) -> bool {
    // The opening quote, spaces, `(`, then the function's name.
    let head = text[..start.saturating_sub(1)].trim_end_matches(' ');
    let Some(head) = head.strip_suffix('(') else {
        return false;
    };
    let tail = &head.as_bytes()[head.len().saturating_sub(32)..];
    let lower = tail.to_ascii_lowercase();
    lower.ends_with(b"setenvironmentvariable") || lower.ends_with(b"setenv")
}

/// The value of `name:` when it stands alone on the next line, indented deeper than the name's
/// line (`password:\n  value`). `newline` is where the name's line ends. Not a mapping
/// (`sub: x`), a list item or a comment.
fn yaml_next_line(text: &str, name_start: usize, newline: usize) -> Option<(usize, usize)> {
    if text.as_bytes().get(newline) != Some(&b'\n') {
        return None;
    }
    let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    let line_start = text[..name_start].rfind('\n').map_or(0, |at| at + 1);
    let own = indent(&text[line_start..name_start]);
    let start = newline + 1;
    let end = text[start..].find('\n').map_or(text.len(), |e| start + e);
    let line = &text[start..end];
    let value = line.trim();
    let scalar = !value.is_empty()
        && !value.contains(": ")
        && !value.ends_with(':')
        && !value.starts_with(['-', '#', '|', '>']);
    (scalar && indent(line) > own).then(|| (start + indent(line), start + line.trim_end().len()))
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
    // Microsoft Teams incoming webhooks; their path holds `@`.
    "webhook.office.com/webhookb2/",
    "outlook.office.com/webhook/",
];

/// Slack, Discord and Teams webhook URLs: the path after the fixed part.
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
                .take_while(|b| is_token_byte(**b) || **b == b'@')
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
/// the text of an element with a secret-looking name, or one whose `key` or `name` attribute
/// names a secret (`<string name="api_key">...</string>`), when a closing tag follows it. Loose
/// names stay (a plist names every entry in `<key>`).
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
        // The rest of the start tag on the same line; a self-closing tag holds nothing. The
        // search stops at the next `<`, so the pass stays linear.
        let Some(close) = text[name_end..]
            .find(['>', '<', '\n'])
            .map(|e| name_end + e)
        else {
            continue;
        };
        if bytes[close] != b'>' || bytes[close - 1] == b'/' {
            continue;
        }
        // `<string name="api_key">`, `<entry key="password">`: the attribute names the entry.
        let secret = named_secret(local)
            || tag_attributes(text, name_end, close)
                .iter()
                .any(|(attribute, (start, end))| {
                    matches!(attribute.as_str(), "key" | "name")
                        && named_secret(&text[*start..*end])
                });
        if !secret {
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

/// A tag or attribute value that names a secret; loose names stay (a plist names every entry in
/// `<key>`).
fn named_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !LOOSE_NAMES.contains(&lower.as_str()) && is_secret_name(&lower)
}

/// Attributes of a start tag: lower-case name and the byte range of the value inside its quotes.
fn tag_attributes(text: &str, from: usize, end: usize) -> Vec<(String, (usize, usize))> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = from;
    while at < end {
        if !(is_name_byte(bytes[at]) || bytes[at] == b':') {
            at += 1;
            continue;
        }
        let name_start = at;
        while at < end && (is_name_byte(bytes[at]) || bytes[at] == b':') {
            at += 1;
        }
        let name = text[name_start..at].to_ascii_lowercase();
        while at < end && bytes[at] == b' ' {
            at += 1;
        }
        if at >= end || bytes[at] != b'=' {
            continue;
        }
        at += 1;
        while at < end && bytes[at] == b' ' {
            at += 1;
        }
        let Some(&quote @ (b'"' | b'\'')) = bytes.get(at).filter(|_| at < end) else {
            continue;
        };
        let value_start = at + 1;
        let Some(close) = text[value_start..end].find(char::from(quote)) else {
            break;
        };
        found.push((name, (value_start, value_start + close)));
        at = value_start + close + 1;
    }
    found
}

/// The `value` attribute of a config entry that names a secret: `<add key="ApiKey" value="..."/>`
/// (.NET `appSettings`, also with `name=`), or an element named like one
/// (`<secret value="..."/>`).
fn redact_xml_attributes(text: &str) -> String {
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
        if name_end == from {
            continue;
        }
        // The rest of the start tag on the same line; stops at the next `<`, so the pass stays
        // linear.
        let Some(close) = text[name_end..]
            .find(['>', '<', '\n'])
            .map(|e| name_end + e)
            .filter(|close| bytes[*close] == b'>')
        else {
            continue;
        };
        let tag = text[from..name_end].to_ascii_lowercase();
        let local = tag.rsplit(':').next().unwrap_or(&tag);
        let attributes = tag_attributes(text, name_end, close);
        let secret = named_secret(local)
            || attributes.iter().any(|(name, (start, end))| {
                matches!(name.as_str(), "key" | "name") && named_secret(&text[*start..*end])
            });
        if !secret {
            continue;
        }
        for (name, (start, end)) in attributes {
            if name == "value" && end > start {
                ranges.push((start, end));
            }
        }
    }
    mask_ranges(text, &ranges)
}

/// Lines that set a variable without `=`: `setx NAME value`, fish `set -x NAME value`, a
/// Dockerfile `ENV NAME value`, and a line of just a secret name and its value
/// (`X-Api-Key <value>`, `token<TAB><value>`). The two-word line needs a value that looks like a
/// token, so `password reset` stays.
fn redact_word_lines(text: &str) -> String {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let list = words(line, offset);
        let line_end = offset + line.trim_end().len();
        offset += line.len();
        let word = |index: usize| list.get(index).map(|&(start, end)| &text[start..end]);
        let Some(first) = word(0) else {
            continue;
        };
        let secret_name = |name: &str| {
            name.bytes().all(is_name_byte) && is_secret_name(&name.to_ascii_lowercase())
        };
        let value_of = |index: usize, name: &str| {
            let (start, end) = unquote(text, *list.get(index)?);
            let bare = name.to_ascii_lowercase();
            (end > start && !harmless_value(text, start, end, bare.trim_start_matches('-')))
                .then_some((start, end))
        };
        // `aws configure set aws_secret_access_key <v>`, `git config --global github.token <v>`,
        // `npm config set //registry.npmjs.org/:_authToken <v>`.
        for index in 1..list.len().saturating_sub(1) {
            let (Some(before), Some(key)) = (word(index - 1), word(index)) else {
                continue;
            };
            if !CONFIG_SETTERS.contains(&before) {
                continue;
            }
            // `set` alone is prose (`set API_KEY to a valid key`); only `config set` and
            // `configure set` take a key.
            let tool_set = index >= 2 && matches!(word(index - 2), Some("config" | "configure"));
            if before == "set" && !tool_set {
                continue;
            }
            let Some(last) = config_key_secret(key) else {
                continue;
            };
            let Some((start, end)) = value_of(index + 1, last) else {
                continue;
            };
            let value = &text[start..end];
            // A plain word after a plain name is prose (`set password reset`); a key spelled as
            // one (`a.b`, `A_B`, `x:y`) or a value that looks like a token is not.
            let spelled = key
                .bytes()
                .any(|b| matches!(b, b'_' | b'.' | b':' | b'/') || b.is_ascii_uppercase());
            let mixed = value.bytes().any(|b| b.is_ascii_digit())
                && value.bytes().any(|b| b.is_ascii_alphabetic());
            let token = mixed && value.len() >= MIN_AUTHORIZATION;
            if !value.starts_with('-') && (spelled || token) {
                ranges.push((start, end));
            }
        }
        match first {
            _ if program(first) == "setx" => {
                if let Some(name) = word(1).filter(|name| secret_name(name)) {
                    ranges.extend(value_of(2, name));
                }
            }
            "set" => {
                let flags = list[1..]
                    .iter()
                    .take_while(|&&(start, _)| text.as_bytes()[start] == b'-')
                    .count();
                let Some(name) = word(1 + flags) else {
                    continue;
                };
                // Without a flag only a variable's spelling counts: `set password reset` is prose.
                let variable = name.contains('_') || !name.bytes().any(|b| b.is_ascii_lowercase());
                if secret_name(name) && (flags > 0 || variable) {
                    ranges.extend(value_of(2 + flags, name));
                }
            }
            "ENV" => {
                if let (Some(name), Some(&(start, _))) = (word(1), list.get(2)) {
                    if secret_name(name) {
                        ranges.push((start, line_end));
                    }
                }
            }
            _ if list.len() == 2 && secret_name(first) => {
                let Some((start, end)) = value_of(1, first) else {
                    continue;
                };
                let value = &text[start..end];
                let token = value.bytes().all(is_token_byte);
                let mixed = value.bytes().any(|b| b.is_ascii_digit())
                    && value.bytes().any(|b| b.is_ascii_alphabetic());
                let loose = LOOSE_NAMES.contains(&first.to_ascii_lowercase().as_str());
                let long = value.len() >= MIN_LOOSE_VALUE;
                if token && (long || (!loose && mixed && value.len() >= MIN_AUTHORIZATION)) {
                    ranges.push((start, end));
                }
            }
            _ => {}
        }
    }
    ranges.sort_unstable();
    mask_ranges(text, &ranges)
}

/// Words after which a config key and its value follow: `aws configure set`, `npm config set`,
/// `git config [--global|--local|--system|--worktree]`. Not `-e`: `grep -e API_KEY src` names a
/// pattern and a path, and a container tool takes `-e NAME=value` (the assignment pass).
const CONFIG_SETTERS: &[&str] = &[
    "set",
    "config",
    "--global",
    "--local",
    "--system",
    "--worktree",
    "--add",
    "--replace-all",
];

/// The last part of a config key (after `.`, `:` or `/`) when it names a secret:
/// `github.token`, `//registry.npmjs.org/:_authToken`, `aws_secret_access_key`.
fn config_key_secret(key: &str) -> Option<&str> {
    let last = key.rsplit(['.', ':', '/']).next()?;
    (!last.is_empty()
        && last.bytes().all(is_name_byte)
        && is_secret_name(&last.to_ascii_lowercase()))
    .then_some(last)
}

/// `Your API key is <value>`: a secret name, `is`, and a value that looks like a token (long,
/// letters and digits), so `the password is wrong` stays.
fn redact_sentences(text: &str) -> String {
    const IS: &str = " is ";
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(IS) {
        let at = from + found;
        from = at + IS.len();
        let name_start = at
            - bytes[..at]
                .iter()
                .rev()
                .take_while(|b| is_name_byte(**b))
                .count();
        if name_start == at || !is_secret_name(&text[name_start..at].to_ascii_lowercase()) {
            continue;
        }
        let quoted = matches!(bytes.get(from), Some(b'"' | b'\'' | b'`'));
        let (start, len) = token_at(bytes, from + usize::from(quoted));
        let mut value = &bytes[start..start + len];
        // A sentence ends after the value.
        while let Some((last, head)) = value.split_last() {
            if !matches!(last, b'.' | b',') {
                break;
            }
            value = head;
        }
        let mixed =
            value.iter().any(u8::is_ascii_digit) && value.iter().any(u8::is_ascii_alphabetic);
        if value.len() >= MIN_LOOSE_VALUE && mixed {
            ranges.push((start, start + value.len()));
            from = start + len;
        }
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

/// A Markdown table row whose cell names a secret (`| DB_PASSWORD | value |`, also in
/// backticks): the next cell. A header row (the line below it separates header and body) names
/// columns, not values, so `| Password | Notes |` stays.
fn redact_table_cells(text: &str) -> String {
    let mut ranges = Vec::new();
    let mut offset = 0;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let line_start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        if !trimmed.starts_with('|') || lines.peek().is_some_and(|next| table_separator(next)) {
            continue;
        }
        let lead = line_start + body.len() - trimmed.len();
        let body_end = line_start + body.len();
        let bytes = trimmed.as_bytes();
        let mut cells = Vec::new();
        let mut cell_start = lead + 1;
        for at in 1..bytes.len() {
            if bytes[at] == b'|' && bytes[at - 1] != b'\\' {
                cells.push((cell_start, lead + at));
                cell_start = lead + at + 1;
            }
        }
        if cell_start < body_end {
            cells.push((cell_start, body_end));
        }
        for pair in cells.windows(2) {
            let (name_start, name_end) = cell_content(text, pair[0]);
            let name = &text[name_start..name_end];
            let lower = name.to_ascii_lowercase();
            if name.is_empty() || !name.bytes().all(is_name_byte) || !is_secret_name(&lower) {
                continue;
            }
            let (start, end) = cell_content(text, pair[1]);
            let bare = lower.trim_start_matches('-');
            if end <= start || harmless_value(text, start, end, bare) {
                continue;
            }
            let token_len = text[start..end]
                .bytes()
                .take_while(|b| is_token_byte(*b))
                .count();
            if LOOSE_NAMES.contains(&bare) && token_len < MIN_LOOSE_VALUE {
                continue;
            }
            ranges.push((start, end));
        }
    }
    mask_ranges(text, &ranges)
}

/// The line between a Markdown table's header and its body: `|---|:--:|`.
fn table_separator(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|')
        && line.contains('-')
        && line
            .bytes()
            .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

/// A table cell without its spaces and the backticks or quotes around it.
fn cell_content(text: &str, (start, end): (usize, usize)) -> (usize, usize) {
    let cell = &text[start..end];
    let inner = cell.trim();
    let from = start + (cell.len() - cell.trim_start().len());
    let to = from + inner.len();
    let quoted = inner.len() >= 2
        && matches!(inner.as_bytes()[0], b'`' | b'"' | b'\'')
        && inner.as_bytes()[inner.len() - 1] == inner.as_bytes()[0];
    if quoted {
        (from + 1, to - 1)
    } else {
        (from, to)
    }
}

/// Calls that read an environment variable and take a default as their second argument.
const GETENV_CALLS: &[&str] = &["getenv(", "environ.get(", "env.get(", "env.fetch("];
/// Operators whose right side is the fallback of a missing variable.
const FALLBACK_OPERATORS: &[&str] = &["||", "??", " or "];

/// A literal used when a secret's variable is missing: `os.getenv("DB_PASSWORD", "...")`,
/// `password = getenv("X", "...")`, `process.env.X || '...'`, `... ?? "..."`, `... or "..."`
/// after a secret name on the same line. Only a quoted literal is masked.
fn redact_fallbacks(text: &str) -> String {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let line = line.trim_end_matches(['\n', '\r']);
        let lower = line.to_ascii_lowercase();
        let secret_at = first_secret_name(line);
        // Where the next fallback may start; skipping masked literals keeps the pass linear.
        let mut next_free = 0;
        let mut found = Vec::new();
        for operator in FALLBACK_OPERATORS {
            let mut from = 0;
            while let Some(hit) = lower[from..].find(operator) {
                let at = from + hit;
                from = at + operator.len();
                if secret_at.is_some_and(|secret| secret <= at) {
                    found.push((at, from, false));
                }
            }
        }
        for call in GETENV_CALLS {
            let mut from = 0;
            while let Some(hit) = lower[from..].find(call) {
                let at = from + hit;
                from = at + call.len();
                found.push((at, from, true));
            }
        }
        found.sort_unstable();
        for (at, after, call) in found {
            if at < next_free {
                continue;
            }
            let literal = if call {
                getenv_default(line, after, secret_at.is_some_and(|secret| secret <= at))
            } else {
                quoted_at(line, skip_spaces(line, after))
            };
            if let Some((start, end)) = literal {
                ranges.push((line_start + start, line_start + end));
                next_free = end;
            }
        }
    }
    ranges.sort_unstable();
    mask_ranges(text, &ranges)
}

/// The end of the first word on `line` that names a secret.
fn first_secret_name(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !is_name_byte(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_name_byte(bytes[at]) {
            at += 1;
        }
        if is_secret_name(&line[start..at].to_ascii_lowercase()) {
            return Some(at);
        }
    }
    None
}

/// The inside of a quoted literal at `at` that is not empty.
fn quoted_at(line: &str, at: usize) -> Option<(usize, usize)> {
    if !matches!(line.as_bytes().get(at), Some(b'"' | b'\'')) {
        return None;
    }
    value_at(line, at).filter(|(start, end)| end > start)
}

/// The quoted second argument of a getenv-like call whose arguments start at `args`, when the
/// first one names a secret or a secret name comes before the call (`secret_before`).
fn getenv_default(line: &str, args: usize, secret_before: bool) -> Option<(usize, usize)> {
    let (name_start, name_end) = quoted_at(line, skip_spaces(line, args))?;
    let named = is_secret_name(&line[name_start..name_end].to_ascii_lowercase());
    let comma = skip_spaces(line, name_end + 1);
    if line.as_bytes().get(comma) != Some(&b',') || !(named || secret_before) {
        return None;
    }
    quoted_at(line, skip_spaces(line, comma + 1))
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
/// sides are numbers (`docker run -u 1000:1000`). Also `twine -p X`, `redis-cli -a X`,
/// `ldapsearch -w X` (and `-y`, the password file), `smbclient -U user%X`, `openssl -k X` and
/// `-pass pass:X`, and the password after the share of `net use`.
fn redact_tool_flags(text: &str) -> String {
    const MYSQL: &[&str] = &["mysql", "mysqldump", "mysqladmin", "mysqlimport", "mariadb"];
    const OTHERS: &[&str] = &["twine", "redis-cli", "smbclient", "openssl", "net"];
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let list = words(line, offset);
        offset += line.len();
        let mut tool = String::new();
        // `echo <password> | docker login --password-stdin`: what is echoed.
        let piped = list.first().is_some_and(|&(start, end)| {
            matches!(&text[start..end], "echo" | "printf")
                && list
                    .iter()
                    .any(|&(start, end)| &text[start..end] == "--password-stdin")
        });
        if piped {
            let echoed = list[1..]
                .iter()
                .take_while(|&&(start, end)| &text[start..end] != "|")
                .map(|&value| unquote(text, value));
            ranges.extend(echoed.filter(|(start, end)| end > start));
        }
        // `net use Z: \\host\share <password> /user:name`: the word after the share.
        let mut after_share = false;
        for (index, &(start, end)) in list.iter().enumerate() {
            let word = &text[start..end];
            let next = list.get(index + 1).copied();
            if std::mem::take(&mut after_share) && !word.starts_with('/') && word != "*" {
                ranges.push(unquote(text, (start, end)));
                continue;
            }
            let next_value = next.map(|value| unquote(text, value));
            match word {
                _ if tool == "net use" && (word.starts_with('\\') || word.starts_with("//")) => {
                    after_share = true;
                }
                "-a" if tool == "redis-cli" => ranges.extend(next_value),
                "-w" | "-y" if tool.starts_with("ldap") => ranges.extend(next_value),
                "-k" if tool == "openssl" => ranges.extend(next_value),
                "-pass" | "-passin" | "-passout" if tool == "openssl" => {
                    if let Some((start, end)) = next_value {
                        if text[start..end].starts_with("pass:") && end > start + 5 {
                            ranges.push((start + 5, end));
                        }
                    }
                }
                "-U" | "--user" if tool == "smbclient" => {
                    if let Some((start, end)) = next_value {
                        if let Some(at) = text[start..end].find('%') {
                            if end > start + at + 1 {
                                ranges.push((start + at + 1, end));
                            }
                        }
                    }
                }
                "-p" => {
                    let takes = tool == "sshpass"
                        || tool == "docker login"
                        || tool == "podman login"
                        || tool == "twine";
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
                    } else if tool == "net" && name == "use" {
                        tool = "net use".to_owned();
                    } else if name == "sshpass"
                        || name == "docker"
                        || name == "podman"
                        || name.starts_with("ldap")
                        || MYSQL.contains(&name.as_str())
                        || OTHERS.contains(&name.as_str())
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
                let rest = &bytes[i + prefix.len()..];
                if AWS_KEY_IDS.contains(prefix) {
                    rest.len() >= *min
                        && rest[..*min]
                            .iter()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                } else {
                    rest.iter().take_while(|b| is_token_byte(**b)).count() >= *min
                }
            }
        });
        let telegram = telegram_token_len(&bytes[i..]);
        if hit.is_some() || telegram.is_some() {
            let token = bytes[i..].iter().take_while(|b| is_token_byte(**b)).count();
            let end = i + token.max(telegram.unwrap_or(0));
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

/// The length of a Telegram bot token at the start of `bytes`: 8 to 10 digits, `:AA`, then at
/// least [`MIN_TELEGRAM_TAIL`] letters, digits, `_` or `-`.
fn telegram_token_len(bytes: &[u8]) -> Option<usize> {
    let digits = bytes
        .iter()
        .take(11)
        .take_while(|b| b.is_ascii_digit())
        .count();
    if !(8..=10).contains(&digits) || !bytes[digits..].starts_with(b":AA") {
        return None;
    }
    let from = digits + 3;
    let tail = bytes[from..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        .count();
    (tail >= MIN_TELEGRAM_TAIL).then_some(from + tail)
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
            // A `.env` line keeps `&` in its value; inside a line it still ends one.
            (format!("SECRET={FAKE}&a=b"), "SECRET=[redacted]"),
            (
                format!("url?SECRET={FAKE}&a=b"),
                "url?SECRET=[redacted]&a=b",
            ),
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
            "---- BEGIN x ",
            "BEGIN ",
            "-----END ",
            "<add key=\"password\" value=\"",
            "<secret ",
            "token is ",
            "a is ",
            "password:\n",
            "Private-Lines: 9\n",
            "set -x TOKEN ",
            "\nAPI_KEY=&",
            "'token'] = ",
            "setenv('token',",
            "a:5432:b:c:",
            "<entry key=\"password\">",
            "<string name=\"x\" ",
            "| token | ",
            "|---|\n",
            "token || '",
            "token ?? ",
            "getenv(\"token\", \"",
            "git config token ",
            "-e DB_PASSWORD ",
            "net use \\\\h\\s ",
            "openssl -pass pass:",
            "123456789:AA",
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

    /// An unquoted value of a `.env` line runs to the next whitespace: `&`, `;`, `)`, `}` and
    /// quotes belong to it there.
    #[test]
    fn env_lines_keep_their_whole_value() {
        masks(&[
            (
                format!("API_KEY={FAKE}&x;y)z}}w\"q'r\nnext"),
                "API_KEY=[redacted]\nnext",
            ),
            (
                format!("  export DB_PASSWORD={FAKE}&more tail"),
                "  export DB_PASSWORD=[redacted] tail",
            ),
            (
                format!("  - API_TOKEN={FAKE};x"),
                "  - API_TOKEN=[redacted]",
            ),
            (
                format!("export API_TOKEN={FAKE}; next"),
                "export API_TOKEN=[redacted]; next",
            ),
        ]);
        // Inside a line the old stops still hold.
        keeps(&["TOKEN=5", "let api_key = load_key(path);"]);
    }

    #[test]
    fn more_assignment_forms() {
        masks(&[
            (format!("api_key => '{FAKE}'"), "api_key => '[redacted]'"),
            (
                format!("$c = ['api_key' => '{FAKE}'];"),
                "$c = ['api_key' => '[redacted]'];",
            ),
            (format!("apiKey := \"{FAKE}\""), "apiKey := \"[redacted]\""),
            (
                format!("api_key <- \"{FAKE}\""),
                "api_key <- \"[redacted]\"",
            ),
            (
                format!("os.environ['API_TOKEN'] = '{FAKE}'"),
                "os.environ['API_TOKEN'] = '[redacted]'",
            ),
            (
                format!("[Environment]::SetEnvironmentVariable('API_TOKEN','{FAKE}')"),
                "[Environment]::SetEnvironmentVariable('API_TOKEN','[redacted]')",
            ),
            (
                format!(
                    "[Environment]::SetEnvironmentVariable(\"API_TOKEN\", \"{FAKE}\", \"User\")"
                ),
                "[Environment]::SetEnvironmentVariable(\"API_TOKEN\", \"[redacted]\", \"User\")",
            ),
            (
                format!("setx API_TOKEN {FAKE} /M"),
                "setx API_TOKEN [redacted] /M",
            ),
            (
                format!("set -x API_TOKEN {FAKE}"),
                "set -x API_TOKEN [redacted]",
            ),
            (
                format!("set -gx GITHUB_TOKEN '{FAKE}'"),
                "set -gx GITHUB_TOKEN '[redacted]'",
            ),
            (
                format!("ENV API_TOKEN {FAKE} more"),
                "ENV API_TOKEN [redacted]",
            ),
            (format!("X-Api-Key {FAKE}"), "X-Api-Key [redacted]"),
            (format!("client_secret {FAKE}"), "client_secret [redacted]"),
            (format!("token\t{FAKE}"), "token\t[redacted]"),
            (
                format!("password:\n  {FAKE}\nuser: me"),
                "password:\n  [redacted]\nuser: me",
            ),
        ]);
        keeps(&[
            "fields = ['token', 'name']",
            "set the password later",
            "set password reset",
            "password reset",
            "token expired",
            "Token\t5",
            "key points",
            "X-Api-Key header",
            "password:\n  sub: x",
            "password:\nuser: me",
            "ENV PATH /usr/bin",
            "a => b",
        ]);
    }

    #[test]
    fn config_attributes_name_their_secret() {
        masks(&[
            (
                format!("<add key=\"DbPassword\" value=\"{FAKE}\"/>"),
                "<add key=\"DbPassword\" value=\"[redacted]\"/>",
            ),
            (
                format!("<add name=\"ApiSecret\" value='{FAKE}' />"),
                "<add name=\"ApiSecret\" value='[redacted]' />",
            ),
            (
                format!("<secret value=\"{FAKE}\"/>"),
                "<secret value=\"[redacted]\"/>",
            ),
        ]);
        keeps(&[
            "<add key=\"Theme\" value=\"dark\"/>",
            "<add name=\"Timeout\" value=\"30\" />",
        ]);
    }

    #[test]
    fn more_prefixes_and_webhooks() {
        for prefix in ["xoxc-", "xoxd-", "xoxe-", "xoxe.xoxp-"] {
            let input = format!("use {prefix}{FAKE} now");
            assert_eq!(redact(&input), "use [redacted] now", "{prefix}");
        }
        masks(&[
            (
                "id ASIAFAKEFAKE01234567 now".to_owned(),
                "id [redacted] now",
            ),
            (
                "post https://x.webhook.office.com/webhookb2/0000-FAKE@0000-FAKE/IncomingWebhook/FAKEVALUE/0000 now"
                    .to_owned(),
                "post https://x.webhook.office.com/webhookb2/[redacted] now",
            ),
        ]);
        keeps(&[
            "ASIA_PACIFIC_REGION_ENDPOINT",
            "AKIA_SOMETHING_LONGER_NAME",
            "see https://x.webhook.office.com/webhookb2/",
        ]);
    }

    #[test]
    fn key_blocks_in_other_spellings() {
        assert_eq!(
            redact("---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----\nAAAAFAKE\n---- END SSH2 ENCRYPTED PRIVATE KEY ----\nafter"),
            "[redacted]\nafter"
        );
        assert_eq!(
            redact("x ----- BEGIN PRIVATE KEY -----\nAAAAFAKE\n----- END PRIVATE KEY -----"),
            "x [redacted]"
        );
        let ppk = "PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\nComment: marker\nPublic-Lines: 1\nAAAAPUBLIC\nPrivate-Lines: 2\nFAKEPRIV1\nFAKEPRIV2\nPrivate-MAC: 00ff\n";
        // The first line's name reads like a key's (`...-Key-File-3`), so its harmless value goes
        // too; over-masking is preferred to a leak.
        assert_eq!(
            redact(ppk),
            "PuTTY-User-Key-File-3: [redacted]\nEncryption: none\nComment: marker\nPublic-Lines: 1\nAAAAPUBLIC\nPrivate-Lines: 2\n[redacted]\nPrivate-MAC: 00ff\n"
        );
        keeps(&["-- BEGIN of the KEY section --", "Private-Lines: none"]);
    }

    #[test]
    fn secrets_named_in_a_sentence() {
        masks(&[(
            "Your API key is FAKE0123456789abcdefXYZ, keep it.".to_owned(),
            "Your API key is [redacted], keep it.",
        )]);
        keeps(&[
            "The password is wrong",
            "the key is the parser module",
            "Your API key is stored in the keychain",
            "The token is expired_and_needs_refresh",
        ]);
    }

    #[test]
    fn pgpass_lines() {
        masks(&[
            (
                format!("db.example.invalid:5432:appdb:appuser:{FAKE}"),
                "db.example.invalid:5432:appdb:appuser:[redacted]",
            ),
            (
                format!("*:*:*:postgres:{FAKE}\nnext"),
                "*:*:*:postgres:[redacted]\nnext",
            ),
            (
                format!("  host\\:x:5432:db:me:{FAKE}"),
                "  host\\:x:5432:db:me:[redacted]",
            ),
        ]);
        keeps(&[
            "at 10:30:00:00: done",
            "a:b:c:d:e",
            "host:5432:db:me:",
            "std::io::Error:x:y",
            "time 12:30:45:67:89",
        ]);
    }

    #[test]
    fn xml_entries_named_by_an_attribute() {
        masks(&[
            (
                format!("<string name=\"api_key\">{FAKE}</string>"),
                "<string name=\"api_key\">[redacted]</string>",
            ),
            (
                format!("<entry key=\"password\">{FAKE}</entry>"),
                "<entry key=\"password\">[redacted]</entry>",
            ),
            (
                format!("<setting name='DbPassword' serializeAs=\"String\">{FAKE}</setting>"),
                "<setting name='DbPassword' serializeAs=\"String\">[redacted]</setting>",
            ),
        ]);
        keeps(&[
            "<string name=\"app_name\">Hangar</string>",
            "<entry key=\"theme\">dark</entry>",
            "<string name=\"title\">Settings</string>",
            "<key>password</key>",
        ]);
    }

    #[test]
    fn config_commands_that_set_a_secret() {
        masks(&[
            (
                format!("aws configure set aws_secret_access_key {FAKE}"),
                "aws configure set aws_secret_access_key [redacted]",
            ),
            (
                format!("git config --global github.token {FAKE}"),
                "git config --global github.token [redacted]",
            ),
            (
                format!("git config hub.oauthtoken {FAKE}"),
                "git config hub.oauthtoken [redacted]",
            ),
            (
                format!("npm config set //registry.npmjs.org/:_authToken {FAKE}"),
                "npm config set //registry.npmjs.org/:_authToken [redacted]",
            ),
            (
                format!("yarn config set npmAuthToken {FAKE}"),
                "yarn config set npmAuthToken [redacted]",
            ),
        ]);
        keeps(&[
            "set password reset",
            "git config --global user.name Someone",
            "git config --global credential.helper manager",
            "you can config token values later",
            "grep -e password notes.txt",
            "git config --global --unset github.token",
            "you need to set API_KEY to a valid key",
            "then set OPENAI_API_KEY before running",
            "grep -e API_KEY src/main.rs",
            "docker run -e DB_PASSWORD postgres:16",
        ]);
    }

    #[test]
    fn markdown_table_cells() {
        masks(&[
            (
                format!("| DB_PASSWORD | {FAKE} |"),
                "| DB_PASSWORD | [redacted] |",
            ),
            (
                format!("| Name | Value |\n|---|---|\n| `API_TOKEN` | `{FAKE}` |\n| port | 5432 |"),
                "| Name | Value |\n|---|---|\n| `API_TOKEN` | `[redacted]` |\n| port | 5432 |",
            ),
            (
                "| password | two words |".to_owned(),
                "| password | [redacted] |",
            ),
        ]);
        keeps(&[
            "| Column | Value |",
            "| Password | Notes |\n|---|---|\n| x | y |",
            "| key | Ctrl+K |",
            "| token | 5 |",
            "| TOKEN_COUNT | 12 |",
        ]);
    }

    #[test]
    fn fallback_literals_of_secret_variables() {
        masks(&[
            (
                format!("password = os.getenv(\"DB_PASSWORD\", \"{FAKE}\")"),
                "password = os.getenv(\"DB_PASSWORD\", \"[redacted]\")",
            ),
            (
                format!("x = os.environ.get('API_TOKEN', '{FAKE}')"),
                "x = os.environ.get('API_TOKEN', '[redacted]')",
            ),
            (
                format!("password := getenv(\"X\", \"{FAKE}\")"),
                "password := getenv(\"X\", \"[redacted]\")",
            ),
            (
                format!("DB_PASSWORD = os.getenv(\"DB_PASSWORD\") or \"{FAKE}\""),
                "DB_PASSWORD = os.getenv(\"DB_PASSWORD\") or \"[redacted]\"",
            ),
            (
                format!("const secret = env(\"S\") ?? \"{FAKE}\";"),
                "const secret = env(\"S\") ?? \"[redacted]\";",
            ),
            (
                format!("const t = process.env.API_TOKEN || '{FAKE}'"),
                "const t = process.env.API_TOKEN || '[redacted]'",
            ),
        ]);
        keeps(&[
            "home = os.getenv(\"HOME\", \"/root\")",
            "name = user or \"anonymous\"",
            "const port = process.env.PORT || '8080'",
            "the password or the key",
            "x = os.environ.get('API_TOKEN', '')",
        ]);
    }

    #[test]
    fn password_flags_of_more_tools() {
        masks(&[
            (
                format!("redis-cli -h db -a {FAKE} ping"),
                "redis-cli -h db -a [redacted] ping",
            ),
            (
                format!("ldapsearch -x -D cn=admin -w {FAKE} -b dc=x"),
                "ldapsearch -x -D cn=admin -w [redacted] -b dc=x",
            ),
            (
                "ldapwhoami -x -D cn=admin -y /run/secrets/ldap".to_owned(),
                "ldapwhoami -x -D cn=admin -y [redacted]",
            ),
            (
                format!("smbclient //host.invalid/share -U me%{FAKE}"),
                "smbclient //host.invalid/share -U me%[redacted]",
            ),
            (
                format!("net use Z: \\\\host.invalid\\share {FAKE} /user:me"),
                "net use Z: \\\\host.invalid\\share [redacted] /user:me",
            ),
            (
                format!("openssl enc -aes-256-cbc -k {FAKE} -in x"),
                "openssl enc -aes-256-cbc -k [redacted] -in x",
            ),
            (
                format!("openssl rsa -in k.pem -passout pass:{FAKE} -out y"),
                "openssl rsa -in k.pem -passout pass:[redacted] -out y",
            ),
            (
                format!("twine upload -u __token__ -p {FAKE} dist/*"),
                "twine upload -u __token__ -p [redacted] dist/*",
            ),
            (
                format!("echo {FAKE} | docker login -u me --password-stdin"),
                "echo [redacted] | docker login -u me --password-stdin",
            ),
        ]);
        keeps(&[
            "redis-cli -h db ping",
            "ls -a -w 80",
            "smbclient //host.invalid/share -U me",
            "net use Z: \\\\host.invalid\\share /user:me",
            "net use Z: \\\\host.invalid\\share * /user:me",
            "openssl rsa -in k.pem -passin env:KEY_PASS",
            "echo done | tee log.txt",
            "grep -a -k pattern",
        ]);
    }

    #[test]
    fn newer_token_prefixes() {
        for token in [
            format!("pypi-AgE{FAKE}{FAKE}"),
            format!("whsec_{FAKE}"),
            format!("AGE-SECRET-KEY-1{}", "Q".repeat(58)),
            format!("glsa_{FAKE}{FAKE}"),
            format!("HRKU-{FAKE}"),
            format!("123456789:AA{FAKE}{FAKE}"),
        ] {
            let input = format!("use {token} now");
            assert_eq!(redact(&input), "use [redacted] now", "{token}");
        }
        keeps(&[
            "pypi-AgE short",
            "whsec_short",
            "AGE-SECRET-KEY-1 is the age prefix",
            "glsa_x and HRKU-x",
            "at 123456789:AAA and 12:AA",
            "id 12345678901:AAFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE",
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
