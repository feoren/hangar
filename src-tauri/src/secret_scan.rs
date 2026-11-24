//! Secret guard (ADR 0015): a few precise shapes of keys and tokens that should never travel in a
//! message, a commit or a paste, and file names that usually hold such keys.
//!
//! Pure and small on purpose. The result is only the kinds found ([`SecretKind`]), never the
//! matching text or its position, so nothing that uses it can log or show the secret by accident.
//! Every shape is anchored on a fixed prefix plus a run of the characters that kind uses, with a
//! word boundary in front, so ordinary prose, identifiers and placeholders (`ghp_xxxx...`, the AWS
//! documentation key) do not count. The window has the same shapes in `src/secretScan.ts`; both
//! sides check the shared vectors in `tests/data/secret-scan-vectors.json`.
//!
//! A helper, not a boundary: a key split across lines, encoded or of another kind passes unseen.

use std::borrow::Cow;
use std::ops::Range;

use serde::Serialize;

/// One kind of secret; the window names it in its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    /// A PEM private key block (`-----BEGIN ... PRIVATE KEY-----` with a body).
    PrivateKey,
    /// An AWS access key id (`AKIA`, `ASIA`, `ABIA`, `ACCA` plus 16 characters).
    AwsAccessKey,
    /// A GitHub token (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`).
    GithubToken,
    /// An Anthropic API key (`sk-ant-`).
    AnthropicKey,
    /// An OpenAI API key (`sk-proj-`, `sk-svcacct-`, `sk-admin-`, or a legacy `sk-` key).
    OpenaiKey,
    /// A Slack token (`xoxb-`, `xoxp-`, `xoxa-`, `xoxr-`, `xoxs-`, `xoxo-`, `xapp-`).
    SlackToken,
    /// A Stripe live secret or restricted key (`sk_live_`, `rk_live_`).
    StripeKey,
    /// A staged file whose name usually holds a secret ([`secret_file_name`]); never found in
    /// text, only by the commit check.
    SecretFile,
}

impl SecretKind {
    /// The kind's stable name, as the window receives it.
    pub fn id(self) -> &'static str {
        match self {
            SecretKind::PrivateKey => "privateKey",
            SecretKind::AwsAccessKey => "awsAccessKey",
            SecretKind::GithubToken => "githubToken",
            SecretKind::AnthropicKey => "anthropicKey",
            SecretKind::OpenaiKey => "openaiKey",
            SecretKind::SlackToken => "slackToken",
            SecretKind::StripeKey => "stripeKey",
            SecretKind::SecretFile => "secretFile",
        }
    }

    /// English name for texts the agent reads.
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::PrivateKey => "private key",
            SecretKind::AwsAccessKey => "AWS access key",
            SecretKind::GithubToken => "GitHub token",
            SecretKind::AnthropicKey => "Anthropic API key",
            SecretKind::OpenaiKey => "OpenAI API key",
            SecretKind::SlackToken => "Slack token",
            SecretKind::StripeKey => "Stripe secret key",
            SecretKind::SecretFile => "file that usually holds secrets",
        }
    }
}

/// Characters after a private key header in which its body must start.
const PEM_BODY_WINDOW: usize = 512;
/// Base64 characters in a row that make a key body.
const PEM_BODY_RUN: usize = 40;
/// Longest label between `-----BEGIN ` and `-----`.
const PEM_LABEL_MAX: usize = 96;
/// Longest token body that counts; a longer run of the same characters is no key of these kinds
/// (the window's regular expressions need the same bound, `src/secretScan.ts`).
pub const MAX_TOKEN_BODY: usize = 512;

fn base62(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn word_or_dash(b: u8) -> bool {
    word(b) || b == b'-'
}

fn upper_or_digit(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

/// The end of the run of bytes from `start` that `class` accepts.
fn run_end(bytes: &[u8], start: usize, class: fn(u8) -> bool) -> usize {
    bytes[start..]
        .iter()
        .position(|b| !class(*b))
        .map_or(bytes.len(), |offset| start + offset)
}

/// Whether the byte before `at` is not one `class` accepts (or `at` is the start).
fn boundary_before(bytes: &[u8], at: usize, class: fn(u8) -> bool) -> bool {
    at == 0 || !class(bytes[at - 1])
}

/// A random-looking token body, not a placeholder: enough different characters and none of the
/// words placeholders are made of.
fn plausible(body: &[u8], min_distinct: usize) -> bool {
    let mut seen = [false; 256];
    let mut distinct = 0;
    for b in body {
        if !seen[usize::from(*b)] {
            seen[usize::from(*b)] = true;
            distinct += 1;
        }
    }
    if distinct < min_distinct {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    !["xxxx", "example", "placeholder", "redacted"]
        .iter()
        .any(|marker| lower.windows(marker.len()).any(|w| w == marker.as_bytes()))
}

/// Starts of `prefix` in `text`, as byte offsets.
fn starts<'a>(text: &'a str, prefix: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(prefix).map(|(at, _)| at)
}

/// The bodies of tokens `prefix` + a run of `class` of `min` to [`MAX_TOKEN_BODY`] bytes, with a
/// boundary of `before` in front, the run not followed by a byte of `after`, and a plausible body;
/// as byte ranges after the prefix.
///
/// Linear: the boundary is checked before the run is read, and every `class` lies inside its
/// `before`, so a prefix inside an earlier run fails at once and no byte is read twice by runs.
fn token<'a>(
    text: &'a str,
    prefix: &'a str,
    before: fn(u8) -> bool,
    class: fn(u8) -> bool,
    after: fn(u8) -> bool,
    min: usize,
) -> impl Iterator<Item = Range<usize>> + 'a {
    let bytes = text.as_bytes();
    starts(text, prefix).filter_map(move |at| {
        if !boundary_before(bytes, at, before) {
            return None;
        }
        let body_start = at + prefix.len();
        let end = run_end(bytes, body_start, class);
        let followed = bytes.get(end).is_some_and(|b| after(*b));
        let length = end - body_start;
        (length >= min
            && length <= MAX_TOKEN_BODY
            && !followed
            && plausible(&bytes[body_start..end], 10))
        .then_some(body_start..end)
    })
}

fn nothing(_: u8) -> bool {
    false
}

fn base64(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
}

/// How far after a private key header [`mask`] looks for its `-----END ` line; a 8192 bit RSA key
/// takes about 6.5 KiB, colour sequences between its lines included.
const PEM_MASK_WINDOW: usize = 16 * 1024;

/// The bodies of private key blocks: from the end of the header to the `-----END ` line, or, if
/// none follows within [`PEM_MASK_WINDOW`], to the end of that window. The flag tells whether the
/// end line was found.
fn private_key(text: &str) -> impl Iterator<Item = (Range<usize>, bool)> + '_ {
    let bytes = text.as_bytes();
    // Where the body search of an earlier header stopped: a run that started before it was read
    // whole and was too short, so a later window that overlaps starts after it (linear on text
    // full of headers).
    let mut read_to = 0;
    // The end of the last block found: a header inside it belongs to that block, so no byte is
    // searched for an end line twice.
    let mut block_to = 0;
    starts(text, "-----BEGIN ").filter_map(move |at| {
        if at < block_to {
            return None;
        }
        let label_start = at + "-----BEGIN ".len();
        let label_end = run_end(bytes, label_start, |b| upper_or_digit(b) || b == b' ');
        let label = &text[label_start..label_end];
        if label_end - label_start > PEM_LABEL_MAX
            || !(label.ends_with("PRIVATE KEY") || label.ends_with("PRIVATE KEY BLOCK"))
            || !text[label_end..].starts_with("-----")
        {
            return None;
        }
        let body_start = label_end + "-----".len();
        let window_end = (body_start + PEM_BODY_WINDOW).min(bytes.len());
        let mut index = body_start.max(read_to);
        let mut found = false;
        while index < window_end {
            let end = run_end(bytes, index, base64);
            if end - index >= PEM_BODY_RUN {
                found = true;
                break;
            }
            index = end + 1;
        }
        read_to = read_to.max(index);
        if !found {
            return None;
        }
        let limit = (body_start + PEM_MASK_WINDOW).min(bytes.len());
        let end_line = bytes[body_start..limit]
            .windows(b"-----END ".len())
            .position(|w| w == b"-----END ");
        let block = match end_line {
            Some(offset) => (body_start..body_start + offset, true),
            None => (body_start..limit, false),
        };
        block_to = block.0.end;
        Some(block)
    })
}

/// The bodies of AWS access key ids, after their four letter prefix.
fn aws_access_key(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let bytes = text.as_bytes();
    ["AKIA", "ASIA", "ABIA", "ACCA"]
        .into_iter()
        .flat_map(move |prefix| {
            starts(text, prefix).filter_map(move |at| {
                if !boundary_before(bytes, at, base62) {
                    return None;
                }
                let body_start = at + prefix.len();
                let end = run_end(bytes, body_start, upper_or_digit);
                let body = &bytes[body_start..end];
                (end - body_start == 16
                    && !bytes.get(end).is_some_and(|b| base62(*b))
                    && !body.ends_with(b"EXAMPLE")
                    && plausible(body, 8))
                .then_some(body_start..end)
            })
        })
}

fn github_token(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .into_iter()
        .flat_map(move |prefix| token(text, prefix, word, base62, |b| b == b'_', 36))
        .chain(token(text, "github_pat_", word, word, nothing, 60))
}

fn anthropic_key(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    token(text, "sk-ant-", word_or_dash, word_or_dash, nothing, 40)
}

fn openai_key(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let modern = ["sk-proj-", "sk-svcacct-", "sk-admin-"]
        .into_iter()
        .flat_map(move |prefix| token(text, prefix, word_or_dash, word_or_dash, nothing, 40));
    let bytes = text.as_bytes();
    // Legacy keys: `sk-` and 48 letters and digits with "T3BlbkFJ" (base64 of "OpenAI") inside.
    let legacy = starts(text, "sk-").filter_map(move |at| {
        // The boundary first, so runs never overlap (see `token`).
        if !boundary_before(bytes, at, word_or_dash) {
            return None;
        }
        let body_start = at + 3;
        let end = run_end(bytes, body_start, base62);
        let body = &bytes[body_start..end];
        ((40..=MAX_TOKEN_BODY).contains(&body.len())
            && !bytes.get(end).is_some_and(|b| *b == b'_' || *b == b'-')
            && body.windows(8).any(|w| w == b"T3BlbkFJ")
            && plausible(body, 10))
        .then_some(body_start..end)
    });
    modern.chain(legacy)
}

fn slack_token(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let bytes = text.as_bytes();
    let dash_or_base62 = |b: u8| base62(b) || b == b'-';
    [
        "xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-", "xoxo-", "xapp-",
    ]
    .into_iter()
    .flat_map(move |prefix| {
        starts(text, prefix).filter_map(move |at| {
            // The boundary first, so runs never overlap (see `token`): with the run read first,
            // text made of the prefix again and again took quadratic time.
            if !boundary_before(bytes, at, word_or_dash) {
                return None;
            }
            let body_start = at + prefix.len();
            let end = run_end(bytes, body_start, dash_or_base62);
            let body = &bytes[body_start..end];
            ((20..=MAX_TOKEN_BODY).contains(&body.len())
                && body.contains(&b'-')
                && body.iter().any(u8::is_ascii_digit)
                && plausible(body, 10))
            .then_some(body_start..end)
        })
    })
}

fn stripe_key(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    ["sk_live_", "rk_live_"]
        .into_iter()
        .flat_map(move |prefix| token(text, prefix, word, base62, nothing, 24))
}

/// The token bodies of every kind but private keys, in no particular order.
fn token_bodies(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    aws_access_key(text)
        .chain(github_token(text))
        .chain(anthropic_key(text))
        .chain(openai_key(text))
        .chain(slack_token(text))
        .chain(stripe_key(text))
}

/// The kinds of secret `text` holds, each once, in [`SecretKind`] order; empty for none.
pub fn scan(text: &str) -> Vec<SecretKind> {
    let found = [
        (SecretKind::PrivateKey, private_key(text).next().is_some()),
        (
            SecretKind::AwsAccessKey,
            aws_access_key(text).next().is_some(),
        ),
        (SecretKind::GithubToken, github_token(text).next().is_some()),
        (
            SecretKind::AnthropicKey,
            anthropic_key(text).next().is_some(),
        ),
        (SecretKind::OpenaiKey, openai_key(text).next().is_some()),
        (SecretKind::SlackToken, slack_token(text).next().is_some()),
        (SecretKind::StripeKey, stripe_key(text).next().is_some()),
    ];
    found
        .into_iter()
        .filter(|(_, hit)| *hit)
        .map(|(kind, _)| kind)
        .collect()
}

/// The character a masked secret byte becomes; outside every shape's body class, so a masked
/// text holds no shape any more.
pub const MASK: char = '*';

/// Without an end line, a private key block's window may reach into the text after a cut key;
/// runs of base64 characters shorter than this are left alone there.
const PEM_MASK_RUN: usize = 16;

/// One terminal escape sequence: where it ends, and for a string (OSC, DCS, SOS, PM, APC) where
/// its payload lies between the introducer and the terminator.
struct Escape {
    end: usize,
    payload: Option<Range<usize>>,
}

/// The terminal escape sequence that starts at `at` (an ESC byte), looked for up to `limit`: a
/// CSI ends at its final byte, a string at BEL or ST, any other after its intermediates and one
/// final byte. One without its end reaches `limit`; an ESC before the final byte starts the next
/// sequence, as on a terminal. Every end lies after an ASCII byte or at `limit`, so a sequence
/// covers whole characters.
fn escape(bytes: &[u8], at: usize, limit: usize) -> Escape {
    match bytes.get(at + 1) {
        Some(b'[') => Escape {
            end: (at + 2..limit)
                .find(|&i| bytes[i] == 0x1b || (0x40..=0x7e).contains(&bytes[i]))
                .map_or(limit, |i| if bytes[i] == 0x1b { i } else { i + 1 }),
            payload: None,
        },
        Some(b']' | b'P' | b'X' | b'^' | b'_') => {
            let start = (at + 2).min(limit);
            let terminator = (start..limit).find_map(|i| match bytes[i] {
                0x07 => Some((i, i + 1)),
                0x1b if bytes.get(i + 1) == Some(&b'\\') && i + 1 < limit => Some((i, i + 2)),
                _ => None,
            });
            let (payload_end, end) = terminator.unwrap_or((limit, limit));
            Escape {
                end,
                payload: Some(start..payload_end),
            }
        }
        Some(_) => {
            let end = run_end(&bytes[..limit], (at + 1).min(limit), |b| {
                (0x20..=0x2f).contains(&b)
            });
            // Only a final byte (0x30 to 0x7e) belongs to the sequence; the lead byte of a
            // character or an ESC is left for the text or the next sequence.
            let end = match bytes[..limit].get(end) {
                Some(final_byte) if (0x30..=0x7e).contains(final_byte) => end + 1,
                _ => end,
            };
            Escape { end, payload: None }
        }
        None => Escape {
            end: limit,
            payload: None,
        },
    }
}

/// `text` as the shapes see it on a terminal: every byte of an escape sequence except a string's
/// payload becomes ESC, so a colour sequence right in front of a token (`ESC [ 1 m`, whose final
/// `m` is a word character) is a boundary and no run reaches into a sequence. Offsets stay those
/// of `text`; a link's address in an OSC 8 payload stays visible to the shapes.
fn plain_view(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    if !bytes.contains(&0x1b) {
        return Cow::Borrowed(text);
    }
    let mut plain = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            index += 1;
            continue;
        }
        let found = escape(bytes, index, bytes.len());
        for (offset, byte) in plain[index..found.end].iter_mut().enumerate() {
            let at = index + offset;
            if !found.payload.as_ref().is_some_and(|p| p.contains(&at)) {
                *byte = 0x1b;
            }
        }
        index = found.end;
    }
    // Every replaced byte span starts and ends on an ASCII byte and became ASCII, so the result
    // is UTF-8; should that ever fail, the shapes look at the text as it is.
    String::from_utf8(plain).map_or(Cow::Borrowed(text), Cow::Owned)
}

/// The runs of at least `min` base64 characters in `block` outside terminal escape sequences,
/// strings included, so masking never touches a sequence between the lines of a key.
fn base64_runs(bytes: &[u8], block: Range<usize>, min: usize, out: &mut Vec<Range<usize>>) {
    let mut index = block.start;
    while index < block.end {
        if bytes[index] == 0x1b {
            // Bounded by the block, so blocks full of unterminated strings stay linear.
            index = escape(bytes, index, block.end).end;
            continue;
        }
        if !base64(bytes[index]) {
            index += 1;
            continue;
        }
        let end = run_end(bytes, index, base64).min(block.end);
        if end - index >= min {
            out.push(index..end);
        }
        index = end;
    }
}

/// `text` with the body of every secret [`scan`] finds replaced by [`MASK`], one for each byte;
/// made for terminal text such as a saved scrollback.
///
/// The prefix that names the kind stays (`ghp_****...`), so the reader still sees what was there,
/// and the replacement keeps the body's length: every body byte is ASCII and one terminal column
/// wide, so a restored scrollback keeps its line wraps and cursor moves. Terminal escape
/// sequences stay as they are ([`plain_view`]); a private key block keeps its header and end
/// line. Only full matches are masked, never a part that merely looks like the start of a key.
/// Borrowed when nothing was found.
pub fn mask(text: &str) -> Cow<'_, str> {
    let plain = plain_view(text);
    let mut ranges: Vec<Range<usize>> = token_bodies(&plain).collect();
    for (block, closed) in private_key(&plain) {
        let min = if closed { 1 } else { PEM_MASK_RUN };
        base64_runs(text.as_bytes(), block, min, &mut ranges);
    }
    if ranges.is_empty() {
        return Cow::Borrowed(text);
    }
    ranges.sort_by_key(|range| range.start);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        if range.end <= cursor {
            continue;
        }
        let start = range.start.max(cursor);
        // Every range covers ASCII bytes only, the same in `text` and its plain view, so both of
        // its ends lie on character boundaries.
        out.push_str(&text[cursor..start]);
        out.extend(std::iter::repeat_n(MASK, range.end - start));
        cursor = range.end;
    }
    out.push_str(&text[cursor..]);
    Cow::Owned(out)
}

/// The kinds as an English list for a text the agent reads ("GitHub token, private key").
pub fn labels(kinds: &[SecretKind]) -> String {
    kinds
        .iter()
        .map(|kind| kind.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a file name (the last part of a path, either separator) usually holds a secret: `.env`
/// and `.env.<name>` except the templates `.env.example`, `.env.sample` and `.env.template`;
/// `*.pem`, `*.key`, `*.p12`, `*.pfx`; the SSH private keys `id_rsa`, `id_dsa`, `id_ecdsa`,
/// `id_ed25519`; `credentials.json`, `.netrc`, `_netrc` and `.pgpass`. Case is ignored.
pub fn secret_file_name(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if name == ".env" {
        return true;
    }
    if let Some(rest) = name.strip_prefix(".env.") {
        return !rest.is_empty() && !["example", "sample", "template"].contains(&rest);
    }
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if !stem.is_empty() && ["pem", "key", "p12", "pfx"].contains(&ext) {
            return true;
        }
    }
    [
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        "credentials.json",
        ".netrc",
        "_netrc",
        ".pgpass",
    ]
    .contains(&name.as_str())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One vector of the shared file.
    pub(crate) struct Vector {
        pub name: String,
        pub text: String,
        pub kinds: Vec<String>,
    }

    /// The shared vectors, each text joined from its parts.
    pub(crate) fn vectors() -> (Vec<Vector>, Vec<Vector>) {
        let file: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/secret-scan-vectors.json")).unwrap();
        let read = |list: &serde_json::Value| -> Vec<Vector> {
            list.as_array()
                .unwrap()
                .iter()
                .map(|entry| Vector {
                    name: entry["name"].as_str().unwrap().to_owned(),
                    text: entry["parts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|part| part.as_str().unwrap())
                        .collect(),
                    kinds: entry["kinds"].as_array().map_or_else(Vec::new, |kinds| {
                        kinds
                            .iter()
                            .map(|kind| kind.as_str().unwrap().to_owned())
                            .collect()
                    }),
                })
                .collect()
        };
        (read(&file["hits"]), read(&file["misses"]))
    }

    fn ids(text: &str) -> Vec<String> {
        scan(text).iter().map(|kind| kind.id().to_owned()).collect()
    }

    #[test]
    fn every_hit_of_the_shared_vectors_is_found_with_its_kinds() {
        let (hits, _) = vectors();
        assert!(hits.len() >= 10);
        for vector in hits {
            assert_eq!(ids(&vector.text), vector.kinds, "{}", vector.name);
        }
    }

    #[test]
    fn no_miss_of_the_shared_vectors_is_found() {
        let (_, misses) = vectors();
        assert!(misses.len() >= 10);
        for vector in misses {
            assert_eq!(ids(&vector.text), Vec::<String>::new(), "{}", vector.name);
        }
    }

    #[test]
    fn every_kind_has_a_hit_vector() {
        let (hits, _) = vectors();
        for kind in [
            SecretKind::PrivateKey,
            SecretKind::AwsAccessKey,
            SecretKind::GithubToken,
            SecretKind::AnthropicKey,
            SecretKind::OpenaiKey,
            SecretKind::SlackToken,
            SecretKind::StripeKey,
        ] {
            assert!(
                hits.iter().any(|v| v.kinds == [kind.id()]),
                "{kind:?} has no vector of its own"
            );
        }
    }

    #[test]
    fn the_result_holds_kinds_only_never_the_matched_text() {
        let (hits, _) = vectors();
        for vector in hits {
            let found = scan(&vector.text);
            let shown = format!(
                "{found:?} {} {}",
                serde_json::to_string(&found).unwrap(),
                labels(&found)
            );
            // The longest part is the token body; none of it may appear in what is shown.
            let body = vector
                .text
                .split_whitespace()
                .max_by_key(|w| w.len())
                .unwrap();
            let probe = &body[body.len().saturating_sub(12)..];
            assert!(!shown.contains(probe), "{}: {shown}", vector.name);
        }
    }

    /// Every prefix the shapes look for, and some that start a run of body characters.
    const PREFIXES: &[&str] = &[
        "-----BEGIN ",
        "-----BEGIN PRIVATE KEY-----",
        "AKIA",
        "ASIA",
        "ghp_",
        "gho_",
        "github_pat_",
        "sk-",
        "sk-ant-",
        "sk-proj-",
        "xoxb-",
        "xapp-",
        "sk_live_",
        "rk_live_",
    ];

    // A quadratic scan of 8 MiB would take hours; a linear one takes well under a second in a
    // release build but up to about one second in a debug build on a shared CI runner. The
    // bound therefore leaves room for slow machines and still catches any superlinear shape.
    #[test]
    fn every_shape_stays_linear_on_8_mib_of_one_prefix_repeated() {
        for prefix in PREFIXES {
            for filler in ["", "a", "A1", "-"] {
                let unit = format!("{prefix}{filler}");
                let text = unit.repeat(8 * 1024 * 1024 / unit.len());
                let started = std::time::Instant::now();
                let _ = scan(&text);
                let took = started.elapsed();
                assert!(
                    took < std::time::Duration::from_secs(10),
                    "{unit:?}: {took:?}"
                );
            }
        }
    }

    #[test]
    fn a_body_longer_than_the_bound_is_no_key() {
        let body: String = "Fake0Marker1Value2Test3Hangar4"
            .chars()
            .cycle()
            .take(MAX_TOKEN_BODY + 1)
            .collect();
        assert!(scan(&format!("{}_live_{body}", "sk")).is_empty());
        assert_eq!(
            scan(&format!("{}_live_{}", "sk", &body[..MAX_TOKEN_BODY])),
            [SecretKind::StripeKey]
        );
    }

    /// A made-up base64 body ("Marker fake key body for tests only not a key"), as in the vectors.
    const PEM_BODY: &str = "TWFya2VyIGZha2Uga2V5IGJvZHkgZm9yIHRlc3RzIG9ubHkgbm90IGEga2V5";

    /// A private key header or end line of `label`, joined at run time like the shared vectors,
    /// so no complete key shape is stored in the repository.
    fn pem_line(edge: &str, label: &str) -> String {
        format!("-----{edge} {label}PRIV{}-----", "ATE KEY")
    }

    /// Which bytes differ between a text and its masked form; each must have become [`MASK`].
    fn changed_bytes(text: &str, masked: &str) -> usize {
        assert_eq!(masked.len(), text.len());
        text.bytes()
            .zip(masked.bytes())
            .filter(|(before, after)| {
                if before != after {
                    assert_eq!(*after, MASK as u8);
                }
                before != after
            })
            .count()
    }

    #[test]
    fn every_hit_of_the_shared_vectors_is_masked_in_place() {
        let (hits, _) = vectors();
        for vector in hits {
            let masked = mask(&vector.text);
            assert!(
                changed_bytes(&vector.text, &masked) >= 16,
                "{}",
                vector.name
            );
            assert!(scan(&masked).is_empty(), "{}: {masked}", vector.name);
            // The prefix that names the kind stays readable.
            let probe = &vector.text[..3];
            assert!(masked.starts_with(probe), "{}: {masked}", vector.name);
        }
    }

    #[test]
    fn no_miss_of_the_shared_vectors_is_masked() {
        let (_, misses) = vectors();
        for vector in misses {
            assert!(
                matches!(mask(&vector.text), Cow::Borrowed(text) if text == vector.text),
                "{}",
                vector.name
            );
        }
    }

    #[test]
    fn masking_keeps_the_prefix_and_the_length() {
        let token = format!("{}p_Fake0Marker1Value2Test3Hangar4Only5N", "gh");
        let text = format!("export TOKEN={token} # hangar-test-marker");
        let masked = mask(&text);
        assert_eq!(
            masked,
            format!(
                "export TOKEN={}p_{} # hangar-test-marker",
                "gh",
                "*".repeat(36)
            )
        );
        assert!(matches!(
            mask("nothing here"),
            Cow::Borrowed("nothing here")
        ));
        assert!(matches!(mask(""), Cow::Borrowed("")));
    }

    /// The serialized scrollback carries colour and cursor sequences between the lines of a key;
    /// masking leaves every one of them as it was and keeps the header and end line.
    #[test]
    fn masking_keeps_terminal_escape_sequences_intact() {
        let begin = pem_line("BEGIN", "RSA ");
        let end = pem_line("END", "RSA ");
        let (first, second) = PEM_BODY.split_at(44);
        let link = "\x1b]8;;https://hangar-test.invalid/a\x07link\x1b]8;;\x07";
        let text = format!(
            "\x1b[32m{begin}\x1b[0m\r\n\x1b[38;2;10;200;30m{first}\x1b[0m\r\n\x1b[1m{second}\x1b[m\
             \r\n\x1b[32m{end}\x1b[0m\r\n{link}\x1b[2Kdone"
        );
        let masked = mask(&text);
        assert_eq!(changed_bytes(&text, &masked), PEM_BODY.len());
        assert_eq!(
            masked,
            format!(
                "\x1b[32m{begin}\x1b[0m\r\n\x1b[38;2;10;200;30m{}\x1b[0m\r\n\x1b[1m{}\x1b[m\
                 \r\n\x1b[32m{end}\x1b[0m\r\n{link}\x1b[2Kdone",
                "*".repeat(first.len()),
                "*".repeat(second.len())
            )
        );

        // A token right after a colour sequence (whose final `m` is a word character, no
        // boundary to the plain shapes), and one inside a link's address.
        let token = format!("{}_live_Fake0Marker1Value2Test3Hangar4", "sk");
        let text = format!(
            "\x1b[1m{token}\x1b[0m \x1b]8;;https://hangar-test.invalid/?k={token}\x1b\\x\x1b]8;;\x1b\\"
        );
        assert!(scan(&format!("m{token}")).is_empty());
        let masked = mask(&text);
        let hidden = format!("{}_live_{}", "sk", "*".repeat(30));
        assert_eq!(
            masked,
            format!(
                "\x1b[1m{hidden}\x1b[0m \x1b]8;;https://hangar-test.invalid/?k={hidden}\x1b\\x\x1b]8;;\x1b\\"
            )
        );
        // A sequence cut off at the end of the text changes nothing in front of it.
        let text = format!("{token} \x1b[");
        assert_eq!(mask(&text), format!("{hidden} \x1b["));
    }

    /// An ESC before a character or another ESC ends the sequence there: the plain view stays
    /// UTF-8, so the shapes still see the sequences as boundaries, and the next one is read whole.
    #[test]
    fn a_broken_sequence_leaves_the_next_character_and_sequence_whole() {
        let token = format!("{}_live_Fake0Marker1Value2Test3Hangar4", "sk");
        let hidden = format!("{}_live_{}", "sk", "*".repeat(30));
        for lead in ["\x1b\u{e9}", "\x1b\x1b", "\x1b[\x1b"] {
            let text = format!("{lead}x \x1b[1m{token}\x1b[0m");
            assert_eq!(
                mask(&text),
                format!("{lead}x \x1b[1m{hidden}\x1b[0m"),
                "{lead:?}"
            );
        }
        for lead in ["\x1b\x1b", "\x1b[\x1b"] {
            let text = format!("{lead}[1m{token}");
            assert_eq!(mask(&text), format!("{lead}[1m{hidden}"), "{lead:?}");
        }
    }

    /// Without its end line a key block is masked only in long base64 runs, so the prose after a
    /// cut key stays readable.
    #[test]
    fn a_key_without_its_end_line_masks_only_long_runs() {
        let begin = pem_line("BEGIN", "");
        let text = format!("{begin}\n{PEM_BODY}\n$ ls src\nmain.rs lib.rs\n");
        let masked = mask(&text);
        assert_eq!(
            masked,
            format!(
                "{begin}\n{}\n$ ls src\nmain.rs lib.rs\n",
                "*".repeat(PEM_BODY.len())
            )
        );
        // Two blocks in one text are masked both.
        let block = format!("{begin}\n{PEM_BODY}\n{}\n", pem_line("END", ""));
        let text = format!("{block}between\n{block}");
        let masked = mask(&text);
        assert_eq!(masked.matches(PEM_BODY).count(), 0);
        assert_eq!(masked.matches("between").count(), 1);
        assert_eq!(scan(&masked), []);
    }

    #[test]
    fn masking_stays_linear_on_8_mib_of_one_prefix_repeated() {
        let header = format!("{}\n{}", pem_line("BEGIN", ""), "A1b2".repeat(10));
        for unit in [header.as_str(), "-----BEGIN ", "ghp_a", "sk-A1", "xoxb-"] {
            let text = unit.repeat(8 * 1024 * 1024 / unit.len());
            let started = std::time::Instant::now();
            let masked = mask(&text);
            let took = started.elapsed();
            assert_eq!(masked.len(), text.len());
            assert!(
                took < std::time::Duration::from_secs(10),
                "{unit:?}: {took:?}"
            );
        }
    }

    #[test]
    fn kinds_serialize_as_their_ids() {
        assert_eq!(
            serde_json::to_string(&[SecretKind::PrivateKey, SecretKind::StripeKey]).unwrap(),
            r#"["privateKey","stripeKey"]"#
        );
        assert_eq!(
            labels(&[SecretKind::GithubToken, SecretKind::PrivateKey]),
            "GitHub token, private key"
        );
    }

    #[test]
    fn text_with_non_ascii_characters_is_scanned_without_trouble() {
        assert!(scan("Grüße, ключ: 鍵 -----BEGIN ä").is_empty());
        assert!(scan("").is_empty());
        assert!(scan("sk-").is_empty());
        assert!(scan("-----BEGIN ").is_empty());
    }

    #[test]
    fn secret_file_names() {
        for name in [
            ".env",
            ".env.local",
            ".ENV.production",
            "config/.env",
            r"deploy\server.pem",
            "tls.key",
            "client.p12",
            "cert.PFX",
            "id_rsa",
            "/home/a/.ssh/id_ed25519",
            "id_ecdsa",
            "credentials.json",
            ".netrc",
            "_netrc",
            ".pgpass",
        ] {
            assert!(secret_file_name(name), "{name}");
        }
        for name in [
            ".env.example",
            ".env.sample",
            ".env.template",
            ".envrc",
            "env.ts",
            ".env.",
            "id_rsa.pub",
            "id_ed25519.pub",
            "keys.rs",
            "keyboard.md",
            "pem",
            ".key",
            "credentials.json.example",
            "README.md",
            "src/environment.rs",
        ] {
            assert!(!secret_file_name(name), "{name}");
        }
    }
}
