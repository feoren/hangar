//! The whisper models Hangar offers: pinned file, size and SHA-256, downloaded only after the
//! user agreed, into the app's local data folder. Download, check and removal.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Hugging Face repository revision the files and hashes below belong to
/// (docs/wiki/raw/2026-11-12-dictation-measurement.md). A fixed revision, not `main`, so the
/// same URL always serves the same bytes.
const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const REPOSITORY: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve";
/// Host shown in the consent dialog.
pub const SOURCE_HOST: &str = "huggingface.co";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelId {
    /// whisper base, q5_1: fast, the default (measured 0.5 s for 10 s of speech).
    Base,
    /// whisper small, q5_1: fewer errors, about three times slower.
    Small,
}

pub struct ModelSpec {
    pub id: ModelId,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const MODELS: [ModelSpec; 2] = [
    ModelSpec {
        id: ModelId::Base,
        file: "ggml-base-q5_1.bin",
        bytes: 59_707_625,
        sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
    },
    ModelSpec {
        id: ModelId::Small,
        file: "ggml-small-q5_1.bin",
        bytes: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    },
];

pub fn spec(id: ModelId) -> &'static ModelSpec {
    match id {
        ModelId::Base => &MODELS[0],
        ModelId::Small => &MODELS[1],
    }
}

pub fn url(spec: &ModelSpec) -> String {
    format!("{REPOSITORY}/{REVISION}/{}", spec.file)
}

pub fn path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file)
}

/// Installed means the file is there with the pinned size. The hash is checked on download and
/// once per run before the first load ([`VerifiedModels`]).
pub fn installed(dir: &Path, spec: &ModelSpec) -> bool {
    std::fs::metadata(path(dir, spec)).is_ok_and(|meta| meta.is_file() && meta.len() == spec.bytes)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("The model server could not be reached")]
    Network,
    #[error("The model server answered with status {0}")]
    Http(u16),
    #[error("The downloaded model is not the expected file")]
    Mismatch,
    #[error("The model file could not be written: {0}")]
    Io(String),
}

impl From<std::io::Error> for ModelError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Checks size and hash of a finished download.
pub fn verify(spec: &ModelSpec, bytes: u64, digest: &[u8]) -> Result<(), ModelError> {
    if bytes != spec.bytes || hex(digest) != spec.sha256 {
        return Err(ModelError::Mismatch);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, read in blocks.
fn hash_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut block = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut block)?;
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    Ok(hasher.finalize().to_vec())
}

/// The size and modification time a model file had when its hash last matched.
type Stamp = (u64, SystemTime);

fn stamp(path: &Path) -> std::io::Result<Stamp> {
    let meta = std::fs::metadata(path)?;
    Ok((meta.len(), meta.modified()?))
}

/// Models whose hash matched in this run. whisper.cpp reads a model file as it is; one that
/// changed on disk after the download (another program, a damaged disk) is caught before the
/// first load of each run. Size and modification time stand for "unchanged" afterwards, so the
/// hash (0.2 to 0.6 s) runs once per run and file, not per recording.
#[derive(Default)]
pub struct VerifiedModels(Mutex<HashMap<ModelId, Stamp>>);

impl VerifiedModels {
    /// `Ok` when the file of `spec` in `dir` has the pinned size and hash; hashed only when it
    /// changed since the last match.
    pub fn ensure(&self, dir: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
        let file = path(dir, spec);
        let current = stamp(&file)?;
        if crate::lock(&self.0).get(&spec.id) == Some(&current) {
            return Ok(());
        }
        let digest = hash_file(&file)?;
        verify(spec, current.0, &digest)?;
        crate::lock(&self.0).insert(spec.id, current);
        Ok(())
    }

    /// Marks the file of `spec` as checked, for a download whose hash just matched.
    pub fn mark(&self, dir: &Path, spec: &ModelSpec) {
        if let Ok(current) = stamp(&path(dir, spec)) {
            crate::lock(&self.0).insert(spec.id, current);
        }
    }

    /// Forgets the check, for a removed model.
    pub fn forget(&self, id: ModelId) {
        crate::lock(&self.0).remove(&id);
    }
}

/// Only Hugging Face and its download CDN, https, a few hops.
pub fn client() -> Result<reqwest::Client, ModelError> {
    crate::updates::ensure_crypto_provider();
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        let host = url.host_str().unwrap_or("");
        let allowed = url.scheme() == "https"
            && (host == SOURCE_HOST
                || host.ends_with(".huggingface.co")
                || host.ends_with(".hf.co"))
            && attempt.previous().len() < 5;
        if allowed {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(60))
        .https_only(true)
        .redirect(policy)
        .build()
        .map_err(|_| ModelError::Network)
}

/// Downloads `spec` into `dir`: into a `.part` file first, renamed only after size and hash
/// match. `progress` gets the bytes received so far.
pub async fn download(
    client: &reqwest::Client,
    spec: &ModelSpec,
    dir: &Path,
    progress: impl Fn(u64),
) -> Result<PathBuf, ModelError> {
    std::fs::create_dir_all(dir)?;
    let target = path(dir, spec);
    let part = dir.join(format!("{}.part", spec.file));
    let result = fetch(client, spec, &part, progress).await;
    match result {
        Ok(()) => {
            std::fs::rename(&part, &target)?;
            Ok(target)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

async fn fetch(
    client: &reqwest::Client,
    spec: &ModelSpec,
    part: &Path,
    progress: impl Fn(u64),
) -> Result<(), ModelError> {
    let mut response = client
        .get(url(spec))
        .send()
        .await
        .map_err(|_| ModelError::Network)?;
    if !response.status().is_success() {
        return Err(ModelError::Http(response.status().as_u16()));
    }
    let mut file = std::fs::File::create(part)?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| ModelError::Network)? {
        received += chunk.len() as u64;
        // A larger body is not the pinned file; stop before filling the disk.
        if received > spec.bytes {
            return Err(ModelError::Mismatch);
        }
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        progress(received);
    }
    file.sync_all()?;
    drop(file);
    verify(spec, received, &hasher.finalize())
}

/// Deletes the model file and a leftover partial download.
pub fn remove(dir: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
    for file in [path(dir, spec), dir.join(format!("{}.part", spec.file))] {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join(".tmp")
            .join(format!("dictation-model-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn urls_are_pinned_to_one_revision() {
        for entry in &MODELS {
            let url = url(entry);
            assert!(url.starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/"));
            assert!(url.contains(REVISION), "{url}");
            assert!(url.ends_with(entry.file));
            assert_eq!(entry.sha256.len(), 64);
            assert_eq!(spec(entry.id).file, entry.file);
        }
    }

    #[test]
    fn verify_needs_size_and_hash() {
        let spec = ModelSpec {
            id: ModelId::Base,
            file: "x.bin",
            bytes: 3,
            // SHA-256 of "abc".
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let digest = Sha256::digest(b"abc");
        assert_eq!(verify(&spec, 3, &digest), Ok(()));
        assert_eq!(verify(&spec, 4, &digest), Err(ModelError::Mismatch));
        assert_eq!(
            verify(&spec, 3, &Sha256::digest(b"abd")),
            Err(ModelError::Mismatch)
        );
    }

    /// SHA-256 of "abc", for a three-byte model.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn set_time(file: &Path, time: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    #[test]
    fn a_model_is_hashed_once_until_it_changes() {
        let dir = temp_dir("verified");
        let spec = ModelSpec {
            id: ModelId::Base,
            file: "v.bin",
            bytes: 3,
            sha256: ABC,
        };
        let verified = VerifiedModels::default();
        assert!(matches!(
            verified.ensure(&dir, &spec),
            Err(ModelError::Io(_))
        ));
        let file = path(&dir, &spec);
        std::fs::write(&file, b"abd").unwrap();
        assert_eq!(verified.ensure(&dir, &spec), Err(ModelError::Mismatch));
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(verified.ensure(&dir, &spec), Ok(()));
        // Same size and time: taken as unchanged, not hashed again.
        let time = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::fs::write(&file, b"xyz").unwrap();
        set_time(&file, time);
        assert_eq!(verified.ensure(&dir, &spec), Ok(()));
        // Another time: hashed again, and the changed bytes are caught.
        set_time(&file, time + std::time::Duration::from_secs(5));
        assert_eq!(verified.ensure(&dir, &spec), Err(ModelError::Mismatch));
        // A fresh download marks itself; forgetting makes the next load hash again.
        std::fs::write(&file, b"abc").unwrap();
        verified.mark(&dir, &spec);
        assert_eq!(verified.ensure(&dir, &spec), Ok(()));
        verified.forget(ModelId::Base);
        let time = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::fs::write(&file, b"abz").unwrap();
        set_time(&file, time);
        assert_eq!(verified.ensure(&dir, &spec), Err(ModelError::Mismatch));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_needs_the_exact_size_and_remove_cleans_up() {
        let dir = temp_dir("installed");
        let spec = ModelSpec {
            id: ModelId::Base,
            file: "m.bin",
            bytes: 4,
            sha256: "",
        };
        assert!(!installed(&dir, &spec));
        std::fs::write(path(&dir, &spec), b"abc").unwrap();
        assert!(!installed(&dir, &spec));
        std::fs::write(path(&dir, &spec), b"abcd").unwrap();
        std::fs::write(dir.join("m.bin.part"), b"ab").unwrap();
        assert!(installed(&dir, &spec));
        remove(&dir, &spec).unwrap();
        assert!(!installed(&dir, &spec));
        assert!(!dir.join("m.bin.part").exists());
        // Removing twice is fine.
        remove(&dir, &spec).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
