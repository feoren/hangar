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

/// Whether a token `prefix` + a run of `class` of `min` to [`MAX_TOKEN_BODY`] bytes appears, with a
/// boundary of `before` in front, the run not followed by a byte of `after`, and a plausible body.
///
/// Linear: the boundary is checked before the run is read, and every `class` lies inside its
/// `before`, so a prefix inside an earlier run fails at once and no byte is read twice by runs.
fn token(
    text: &str,
    prefix: &str,
    before: fn(u8) -> bool,
    class: fn(u8) -> bool,
    after: fn(u8) -> bool,
    min: usize,
) -> bool {
    let bytes = text.as_bytes();
    starts(text, prefix).any(|at| {
        if !boundary_before(bytes, at, before) {
            return false;
        }
        let body_start = at + prefix.len();
        let end = run_end(bytes, body_start, class);
        let followed = bytes.get(end).is_some_and(|b| after(*b));
        let length = end - body_start;
        length >= min
            && length <= MAX_TOKEN_BODY
            && !followed
            && plausible(&bytes[body_start..end], 10)
    })
}

fn nothing(_: u8) -> bool {
    false
}

fn private_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    // Where the body search of an earlier header stopped: a run that started before it was read
    // whole and was too short, so a later window that overlaps starts after it (linear on text
    // full of headers).
    let mut read_to = 0;
    starts(text, "-----BEGIN ").any(|at| {
        let label_start = at + "-----BEGIN ".len();
        let label_end = run_end(bytes, label_start, |b| upper_or_digit(b) || b == b' ');
        let label = &text[label_start..label_end];
        if label_end - label_start > PEM_LABEL_MAX
            || !(label.ends_with("PRIVATE KEY") || label.ends_with("PRIVATE KEY BLOCK"))
            || !text[label_end..].starts_with("-----")
        {
            return false;
        }
        let body_start = label_end + "-----".len();
        let window_end = (body_start + PEM_BODY_WINDOW).min(bytes.len());
        let mut index = body_start.max(read_to);
        while index < window_end {
            let end = run_end(bytes, index, |b| {
                b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
            });
            if end - index >= PEM_BODY_RUN {
                return true;
            }
            index = end + 1;
        }
        read_to = read_to.max(index);
        false
    })
}

fn aws_access_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    ["AKIA", "ASIA", "ABIA", "ACCA"].iter().any(|prefix| {
        starts(text, prefix).any(|at| {
            if !boundary_before(bytes, at, base62) {
                return false;
            }
            let body_start = at + prefix.len();
            let end = run_end(bytes, body_start, upper_or_digit);
            let body = &bytes[body_start..end];
            end - body_start == 16
                && !bytes.get(end).is_some_and(|b| base62(*b))
                && !body.ends_with(b"EXAMPLE")
                && plausible(body, 8)
        })
    })
}

fn github_token(text: &str) -> bool {
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|prefix| token(text, prefix, word, base62, |b| b == b'_', 36))
        || token(text, "github_pat_", word, word, nothing, 60)
}

fn anthropic_key(text: &str) -> bool {
    token(text, "sk-ant-", word_or_dash, word_or_dash, nothing, 40)
}

fn openai_key(text: &str) -> bool {
    let modern = ["sk-proj-", "sk-svcacct-", "sk-admin-"]
        .iter()
        .any(|prefix| token(text, prefix, word_or_dash, word_or_dash, nothing, 40));
    let bytes = text.as_bytes();
    // Legacy keys: `sk-` and 48 letters and digits with "T3BlbkFJ" (base64 of "OpenAI") inside.
    let legacy = starts(text, "sk-").any(|at| {
        // The boundary first, so runs never overlap (see `token`).
        if !boundary_before(bytes, at, word_or_dash) {
            return false;
        }
        let body_start = at + 3;
        let end = run_end(bytes, body_start, base62);
        let body = &bytes[body_start..end];
        (40..=MAX_TOKEN_BODY).contains(&body.len())
            && !bytes.get(end).is_some_and(|b| *b == b'_' || *b == b'-')
            && body.windows(8).any(|w| w == b"T3BlbkFJ")
            && plausible(body, 10)
    });
    modern || legacy
}

fn slack_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    let dash_or_base62 = |b: u8| base62(b) || b == b'-';
    [
        "xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-", "xoxo-", "xapp-",
    ]
    .iter()
    .any(|prefix| {
        starts(text, prefix).any(|at| {
            // The boundary first, so runs never overlap (see `token`): with the run read first,
            // text made of the prefix again and again took quadratic time.
            if !boundary_before(bytes, at, word_or_dash) {
                return false;
            }
            let body_start = at + prefix.len();
            let end = run_end(bytes, body_start, dash_or_base62);
            let body = &bytes[body_start..end];
            (20..=MAX_TOKEN_BODY).contains(&body.len())
                && body.contains(&b'-')
                && body.iter().any(u8::is_ascii_digit)
                && plausible(body, 10)
        })
    })
}

fn stripe_key(text: &str) -> bool {
    ["sk_live_", "rk_live_"]
        .iter()
        .any(|prefix| token(text, prefix, word, base62, nothing, 24))
}

/// One shape's check.
type Check = fn(&str) -> bool;

/// The kinds of secret `text` holds, each once, in [`SecretKind`] order; empty for none.
pub fn scan(text: &str) -> Vec<SecretKind> {
    let checks: [(SecretKind, Check); 7] = [
        (SecretKind::PrivateKey, private_key),
        (SecretKind::AwsAccessKey, aws_access_key),
        (SecretKind::GithubToken, github_token),
        (SecretKind::AnthropicKey, anthropic_key),
        (SecretKind::OpenaiKey, openai_key),
        (SecretKind::SlackToken, slack_token),
        (SecretKind::StripeKey, stripe_key),
    ];
    checks
        .iter()
        .filter(|(_, check)| check(text))
        .map(|(kind, _)| *kind)
        .collect()
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
