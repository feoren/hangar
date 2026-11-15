//! Verifies a Tauri updater signature the way installed copies of Hangar do, before a release is
//! published. `tools/release.py` builds it before `tauri build` (`cargo build --locked --offline
//! --example verify_update_sig`), notes its SHA-256 and runs the built program after `tauri signer
//! sign`, only if that hash is unchanged and without the key in its environment:
//!
//! ```text
//! verify_update_sig <file> <file.sig> <pubkey> <version>
//! ```
//!
//! `<pubkey>` is `plugins.updater.pubkey` from tauri.conf.json and `<file.sig>` the file the signer
//! wrote, both base64 of minisign's text form; `<version>` is the version latest.json announces.
//! Mirrors `verify_signature` and `verify_signed_version` of tauri-plugin-updater 2.14.1
//! (src/updater.rs) with `requireSignedVersion` on: the same crate and call verify the signature
//! over the file and the global signature over the trusted comment, and only then is the comment's
//! `version:` field read. The version has to match exactly (the plugin compares as SemVer, which
//! for the X.Y.Z+Build strings release.py writes is the same).

use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};

#[derive(Debug, PartialEq, Eq)]
enum VerifyError {
    /// Not base64 of UTF-8 text.
    Encoding(&'static str),
    /// Not minisign's text form.
    Format(&'static str),
    /// The signature over the file or the global signature over the trusted comment is wrong.
    Signature,
    MissingVersion,
    VersionMismatch {
        signed: String,
        expected: String,
    },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encoding(what) => write!(f, "the {what} is not base64 of UTF-8 text"),
            Self::Format(what) => write!(f, "the {what} is not a minisign {what}"),
            Self::Signature => write!(
                f,
                "the signature does not match the file or the public key, or its trusted comment was changed"
            ),
            Self::MissingVersion => write!(
                f,
                "the trusted comment carries no version (sign with --app-version); the app requires one"
            ),
            Self::VersionMismatch { signed, expected } => write!(
                f,
                "the signature is bound to version {signed}, but the release announces {expected}"
            ),
        }
    }
}

fn decode_text(what: &'static str, encoded: &str) -> Result<String, VerifyError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| VerifyError::Encoding(what))?;
    String::from_utf8(bytes).map_err(|_| VerifyError::Encoding(what))
}

/// The `version` field of a trusted comment: tab separated `key:value` pairs written by the Tauri
/// CLI, e.g. `timestamp:1700000000\tfile:app.exe\tversion:1.2.3`.
fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
}

fn verify(
    data: &[u8],
    signature_base64: &str,
    pubkey_base64: &str,
    expected_version: &str,
) -> Result<(), VerifyError> {
    let public_key = PublicKey::decode(&decode_text("public key", pubkey_base64)?)
        .map_err(|_| VerifyError::Format("public key"))?;
    let signature = Signature::decode(&decode_text("signature", signature_base64)?)
        .map_err(|_| VerifyError::Format("signature"))?;
    // `true` accepts legacy (not prehashed) signatures, exactly as the plugin does.
    public_key
        .verify(data, &signature, true)
        .map_err(|_| VerifyError::Signature)?;
    // Only after `verify`: the global signature covers the trusted comment.
    match signed_version(signature.trusted_comment()) {
        None => Err(VerifyError::MissingVersion),
        Some(signed) if signed == expected_version => Ok(()),
        Some(signed) => Err(VerifyError::VersionMismatch {
            signed: signed.to_owned(),
            expected: expected_version.to_owned(),
        }),
    }
}

fn run(file: &Path, signature_file: &Path, pubkey: &str, version: &str) -> Result<(), String> {
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = std::fs::read_to_string(signature_file)
        .map_err(|e| format!("{}: {e}", signature_file.display()))?;
    verify(&data, &signature, pubkey, version).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, signature_file, pubkey, version] = args.as_slice() else {
        eprintln!("usage: verify_update_sig <file> <file.sig> <pubkey base64> <version>");
        return ExitCode::from(2);
    };
    match run(Path::new(file), Path::new(signature_file), pubkey, version) {
        Ok(()) => {
            println!("Updater signature verified for version {version}: {file}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ERROR: updater signature of {file}: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // FAKE test vectors: a throwaway key pair made once with `tauri signer generate` (Tauri CLI
    // 2.12.4), never the Hangar updater key; its private half was deleted. The untrusted comments
    // were rewritten to say so (they are not covered by any signature). The signatures were made
    // with `tauri signer sign` over PAYLOAD, with and without `--app-version 1.2.3+45`.
    const PAYLOAD: &[u8] = b"FAKE update payload for the signature verification test\n";
    const PUBLIC_KEY: &str =
        "untrusted comment: FAKE throwaway test key, not the Hangar updater key\n\
        RWSN00dgkEssJGBhvsUYjS3shFFnL0hPzFny5fxF6EeyIWM3rmYMoOZ5";
    const VERSIONED: &str = "untrusted comment: FAKE signature from the throwaway test key\n\
        RUSN00dgkEssJBhlVf7Yzy231KBeTF1+O16anQv/Vc0K3ErVsRE3+uUKM2KeiufWcidOawo/zFt28zg53I2W3Qrml/5rNVJZJAM=\n\
        trusted comment: timestamp:1794705459\tfile:payload.txt\tversion:1.2.3+45\n\
        zAgQTSfGUJanwVhQ/P96imzi2WVtMEFGoSuDsEcudgIuR03JCl7Gm6Ep0w+j2sG/rQmXq1sC0DSReLlVRqm3Cw==";
    const UNVERSIONED: &str = "untrusted comment: FAKE signature from the throwaway test key\n\
        RUSN00dgkEssJGeL9ayczqgVKeadM361JOcSTAXQ5AC2RZZWpeWNpE8iy25/EnwhLW//uTWARIjTR0PxR11w7iSi9JQ7f/znhQk=\n\
        trusted comment: timestamp:1794705459\tfile:payload.txt\n\
        PCQv05ZT9RSLAW9JfcBQkTKsVGnzQM8PZ1sjj8Krz41LEng7h6k9V1blzOAbMXjxteS+4hTo6sgGV01LOYnOBA==";
    // The public test key of minisign-verify's documentation: a different key.
    const OTHER_PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\n\
        RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";

    fn encode(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    #[test]
    fn a_valid_signature_bound_to_the_version_verifies() {
        assert_eq!(
            verify(PAYLOAD, &encode(VERSIONED), &encode(PUBLIC_KEY), "1.2.3+45"),
            Ok(())
        );
    }

    #[test]
    fn a_changed_file_is_refused() {
        let mut data = PAYLOAD.to_vec();
        data[0] ^= 1;
        assert_eq!(
            verify(&data, &encode(VERSIONED), &encode(PUBLIC_KEY), "1.2.3+45"),
            Err(VerifyError::Signature)
        );
    }

    #[test]
    fn another_key_is_refused() {
        assert_eq!(
            verify(
                PAYLOAD,
                &encode(VERSIONED),
                &encode(OTHER_PUBLIC_KEY),
                "1.2.3+45"
            ),
            Err(VerifyError::Signature)
        );
    }

    #[test]
    fn a_signature_without_version_is_refused() {
        assert_eq!(
            verify(
                PAYLOAD,
                &encode(UNVERSIONED),
                &encode(PUBLIC_KEY),
                "1.2.3+45"
            ),
            Err(VerifyError::MissingVersion)
        );
    }

    #[test]
    fn another_version_is_refused() {
        assert_eq!(
            verify(PAYLOAD, &encode(VERSIONED), &encode(PUBLIC_KEY), "1.2.4+45"),
            Err(VerifyError::VersionMismatch {
                signed: "1.2.3+45".to_owned(),
                expected: "1.2.4+45".to_owned(),
            })
        );
    }

    #[test]
    fn a_rewritten_trusted_comment_is_refused() {
        // The global signature covers the comment: a forged version must not pass.
        let forged = VERSIONED.replace("version:1.2.3+45", "version:9.9.9+45");
        assert_eq!(
            verify(PAYLOAD, &encode(&forged), &encode(PUBLIC_KEY), "9.9.9+45"),
            Err(VerifyError::Signature)
        );
    }

    #[test]
    fn malformed_input_is_refused() {
        assert_eq!(
            verify(PAYLOAD, "not base64!", &encode(PUBLIC_KEY), "1.2.3+45"),
            Err(VerifyError::Encoding("signature"))
        );
        assert_eq!(
            verify(PAYLOAD, &encode(VERSIONED), &encode("one line"), "1.2.3+45"),
            Err(VerifyError::Format("public key"))
        );
    }

    #[test]
    fn files_are_read_from_paths_with_spaces() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/.tmp")
            .join(format!("verify update sig {}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("Hangar setup.exe");
        let signature = folder.join("Hangar setup.exe.sig");
        std::fs::write(&file, PAYLOAD).unwrap();
        std::fs::write(&signature, encode(VERSIONED)).unwrap();
        let ok = run(&file, &signature, &encode(PUBLIC_KEY), "1.2.3+45");
        let missing = run(
            &folder.join("missing.exe"),
            &signature,
            &encode(PUBLIC_KEY),
            "1.2.3+45",
        );
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(ok, Ok(()));
        assert!(missing.is_err());
    }
}
