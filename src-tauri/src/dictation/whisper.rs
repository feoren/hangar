//! Speech to text with whisper.cpp. The model is loaded per transcription and dropped right
//! after: loading takes 0.1 to 0.3 s (measured), keeping it would hold 250 to 500 MB of RAM
//! for a feature used now and then.

use std::path::Path;
use std::sync::Once;

use serde::{Deserialize, Serialize};

/// Language of the speech. `Auto` lets whisper detect it per recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    #[default]
    Auto,
    De,
    En,
}

impl Language {
    fn code(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::De => Some("de"),
            Self::En => Some("en"),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum TranscribeError {
    #[error("The speech model could not be loaded")]
    Load,
    #[error("Speech recognition failed")]
    Failed,
}

/// Turns 16 kHz mono audio into text. Behind a trait so the service can be tested without a
/// model, and the engine swapped (a whisper.cpp fork, an OS recognizer) without touching it.
pub trait Transcriber: Send + Sync {
    fn transcribe(
        &self,
        model: &Path,
        audio: &[f32],
        language: Language,
    ) -> Result<String, TranscribeError>;
}

pub struct Whisper;

/// Threads for whisper: half the logical cores (about the physical ones), at most 8. Measured: 8
/// threads take 35 % less time than 4 on a 12-core CPU; more would only fight the terminals.
pub fn threads() -> i32 {
    let logical = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    i32::try_from((logical / 2).clamp(1, 8)).unwrap_or(4)
}

static LOGGING: Once = Once::new();

impl Transcriber for Whisper {
    fn transcribe(
        &self,
        model: &Path,
        audio: &[f32],
        language: Language,
    ) -> Result<String, TranscribeError> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
        // Without a logging feature this swallows whisper.cpp's and ggml's console output.
        LOGGING.call_once(whisper_rs::install_logging_hooks);
        let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|_| TranscribeError::Load)?;
        let mut state = context
            .create_state()
            .map_err(|_| TranscribeError::Failed)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(threads());
        params.set_language(language.code());
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state
            .full(params, audio)
            .map_err(|_| TranscribeError::Failed)?;
        let mut text = String::new();
        for segment in state.as_iter() {
            if let Ok(part) = segment.to_str_lossy() {
                text.push_str(&part);
            }
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_stay_in_range() {
        assert!((1..=8).contains(&threads()));
    }

    #[test]
    fn languages_map_to_whisper_codes() {
        assert_eq!(Language::Auto.code(), None);
        assert_eq!(Language::De.code(), Some("de"));
        assert_eq!(Language::En.code(), Some("en"));
        assert_eq!(serde_json::to_string(&Language::De).unwrap(), "\"de\"");
    }

    /// Transcribes a real recording with a real model. Needs both, so it only runs on request:
    /// `HANGAR_WHISPER_MODEL=<model.bin> HANGAR_WHISPER_WAV=<16 kHz mono 16-bit WAV>
    /// cargo test whisper_transcribes -- --ignored`.
    #[test]
    #[ignore = "needs a whisper model and a WAV file (env vars)"]
    fn whisper_transcribes_a_recording() {
        let model = std::env::var("HANGAR_WHISPER_MODEL").expect("HANGAR_WHISPER_MODEL");
        let wav = std::fs::read(std::env::var("HANGAR_WHISPER_WAV").expect("HANGAR_WHISPER_WAV"))
            .unwrap();
        let data = wav
            .windows(4)
            .position(|w| w == b"data")
            .expect("data chunk");
        let audio: Vec<f32> = wav[data + 8..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
            .collect();
        assert!(crate::dictation::audio::has_speech(&audio));
        let text = Whisper
            .transcribe(Path::new(&model), &audio, Language::Auto)
            .unwrap();
        let text = crate::dictation::clean_transcript(&text);
        eprintln!("transcript: {text}");
        assert!(!text.is_empty());
    }
}
