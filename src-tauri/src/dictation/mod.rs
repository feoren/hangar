//! Local dictation: hold a key, speak, the text lands in the session's prompt. Recognition runs
//! on this machine (whisper.cpp); the only network use is the model download the user agreed to.
//! Audio stays in memory, is capped at 60 s, wiped after use and never logged or stored; the
//! transcript waits for the main window in a small queue and is handed out once.
//!
//! The window asks for the text to go into the session it named on key down; it pastes it there
//! through its usual paste path (bracketed paste, control characters stripped), without Enter.

pub mod audio;
pub mod model;
pub mod whisper;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Serialize, Serializer};

use crate::lock;
use crate::session::SessionId;
use audio::{AudioSource, Capture, CaptureError, Recording};
use model::{ModelError, ModelId, VerifiedModels};
use whisper::{Language, Transcriber};

/// Events to the main window. Status and text are bare signals: a listener in any window receives
/// events emitted to the main one, so the state and the dictated text are read through
/// `dictation_status` and `dictation_take_text`, which only the main window may call. The level
/// carries one number from 0 to 1 (see `docs/wiki/pages/syntheses/local-dictation.md`).
pub const STATUS_EVENT: &str = "dictation-status";
pub const LEVEL_EVENT: &str = "dictation-level";
pub const TEXT_EVENT: &str = "dictation-text";

/// Transcripts not yet taken by the window, at most. A window that never asks keeps only the
/// newest; each one is a single recording of at most a minute.
pub const MAX_WAITING_TEXTS: usize = 8;

/// How long a recording may run before the backend stops it on its own: the cap plus a margin. A
/// window reloaded while the key was held never sends its stop.
pub const RECORDING_DEADLINE: Duration = Duration::from_secs(audio::MAX_SECONDS as u64 + 5);

/// What the service is doing. `Idle` is shown as `Ready` or `NotInstalled`, depending on the
/// chosen model ([`DictationState`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Downloading { model: ModelId, progress: f32 },
    Recording { session: SessionId },
    Transcribing { session: SessionId },
    NoMicrophone,
    PermissionDenied,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Install(ModelId),
    Progress(f32),
    Installed,
    InstallFailed(String),
    Removed,
    Record(SessionId),
    CaptureFailed(CaptureError),
    Stop,
    /// Nothing to transcribe (silence, a tap of the key).
    Discard,
    /// The microphone delivered no sound at all.
    Blocked,
    Transcribed,
    TranscribeFailed(String),
}

impl Phase {
    /// Nothing runs: a new download or recording may start.
    fn settled(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::NoMicrophone | Self::PermissionDenied | Self::Failed(_)
        )
    }
}

/// The transitions of [`Phase`]; `None` when `event` is not allowed in `phase`.
pub fn next(phase: &Phase, event: Event) -> Option<Phase> {
    use Phase as P;
    match (phase, event) {
        (p, Event::Install(model)) if p.settled() => Some(P::Downloading {
            model,
            progress: 0.0,
        }),
        (P::Downloading { model, .. }, Event::Progress(progress)) => Some(P::Downloading {
            model: *model,
            progress: progress.clamp(0.0, 1.0),
        }),
        (P::Downloading { .. }, Event::Installed) => Some(P::Idle),
        (P::Downloading { .. }, Event::InstallFailed(message)) => Some(P::Failed(message)),
        (p, Event::Removed) if p.settled() => Some(P::Idle),
        (p, Event::Record(session)) if p.settled() => Some(P::Recording { session }),
        (P::Recording { .. }, Event::CaptureFailed(error)) => Some(match error {
            CaptureError::NoMicrophone => P::NoMicrophone,
            CaptureError::PermissionDenied => P::PermissionDenied,
            CaptureError::Failed(message) => P::Failed(message),
        }),
        (P::Recording { session }, Event::Stop) => Some(P::Transcribing { session: *session }),
        (P::Recording { .. } | P::Transcribing { .. }, Event::Discard) => Some(P::Idle),
        (P::Transcribing { .. }, Event::Blocked) => Some(P::PermissionDenied),
        (P::Transcribing { .. }, Event::Transcribed) => Some(P::Idle),
        (P::Transcribing { .. }, Event::TranscribeFailed(message)) => Some(P::Failed(message)),
        _ => None,
    }
}

/// The state the window shows (Rust and `src/dictation.ts` mirror each other).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DictationState {
    NotInstalled,
    Downloading { model: ModelId, progress: f32 },
    Ready,
    Recording { session: SessionId },
    Transcribing { session: SessionId },
    NoMicrophone,
    PermissionDenied,
    Error { message: String },
}

pub fn view(phase: &Phase, installed: bool) -> DictationState {
    match phase {
        Phase::Idle if installed => DictationState::Ready,
        Phase::Idle => DictationState::NotInstalled,
        Phase::Downloading { model, progress } => DictationState::Downloading {
            model: *model,
            progress: *progress,
        },
        Phase::Recording { session } => DictationState::Recording { session: *session },
        Phase::Transcribing { session } => DictationState::Transcribing { session: *session },
        Phase::NoMicrophone => DictationState::NoMicrophone,
        Phase::PermissionDenied => DictationState::PermissionDenied,
        Phase::Failed(message) => DictationState::Error {
            message: message.clone(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub id: ModelId,
    pub bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStatus {
    /// For the model the window chose last.
    pub state: DictationState,
    pub models: Vec<ModelView>,
    /// Host the models come from, for the consent dialog.
    pub source: &'static str,
    /// The system has an input device. Without one the settings say so instead of offering a
    /// download.
    pub microphone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationText {
    pub session: SessionId,
    pub text: String,
}

/// What the service tells the window. Only the level carries data (a number, see
/// [`STATUS_EVENT`]); the window reads the rest through main-window commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Notice {
    /// The state changed: read `dictation_status`.
    Changed,
    Level(f32),
    /// A transcript waits: read `dictation_take_text`.
    TextReady,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DictationError {
    #[error("Only the main window may do this")]
    WrongWindow,
    #[error("Dictation is busy")]
    Busy,
    #[error("The speech model is not installed")]
    NotInstalled,
    #[error("No such session")]
    UnknownSession,
    #[error("No microphone found")]
    NoMicrophone,
    #[error("Access to the microphone was denied")]
    PermissionDenied,
    #[error("The microphone could not be opened")]
    Microphone,
    #[error("The speech model could not be downloaded")]
    Download,
    #[error("The speech model could not be stored or removed")]
    Storage,
}

impl DictationError {
    /// Stable name for the window, which shows its own translated text.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::WrongWindow => "wrongWindow",
            Self::Busy => "busy",
            Self::NotInstalled => "notInstalled",
            Self::UnknownSession => "unknownSession",
            Self::NoMicrophone => "noMicrophone",
            Self::PermissionDenied => "permissionDenied",
            Self::Microphone => "microphone",
            Self::Download => "download",
            Self::Storage => "storage",
        }
    }
}

impl Serialize for DictationError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.kind())
    }
}

impl From<ModelError> for DictationError {
    fn from(error: ModelError) -> Self {
        match error {
            ModelError::Io(_) => Self::Storage,
            ModelError::Network | ModelError::Http(_) | ModelError::Mismatch => Self::Download,
        }
    }
}

/// The transcript as it goes into a prompt: one line, no control or invisible formatting
/// characters, no whisper annotations such as `[BLANK_AUDIO]`, trimmed.
pub fn clean_transcript(raw: &str) -> String {
    let mut line = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_whitespace() {
            line.push(' ');
        } else if c.is_control() || is_invisible_format(c) {
            continue;
        } else {
            line.push(c);
        }
    }
    line.split(' ')
        .filter(|word| !word.is_empty() && !is_annotation(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bidi controls and zero-width characters: they make text read differently than it runs.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Whisper's own tags for non-speech, all capitals in brackets.
fn is_annotation(word: &str) -> bool {
    let inner = word
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'));
    inner.is_some_and(|inner| {
        !inner.is_empty() && inner.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    })
}

type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

struct Active {
    session: SessionId,
    model: ModelId,
    language: Language,
    capture: Option<Box<dyn Capture>>,
    /// Stop arrived while the microphone was still opening.
    stop_requested: bool,
    /// Tells this recording apart from later ones, for the deadline.
    generation: u64,
}

struct Core {
    phase: Phase,
    selected: ModelId,
    active: Option<Active>,
    /// Last download progress reported, in whole percent.
    reported_percent: u32,
    /// Counts the recordings, for [`Active::generation`].
    recordings: u64,
}

struct Shared {
    models_dir: PathBuf,
    source: Arc<dyn AudioSource>,
    transcriber: Arc<dyn Transcriber>,
    notify: Notify,
    core: Mutex<Core>,
    /// Transcripts the window has not taken yet, oldest first.
    texts: Mutex<VecDeque<DictationText>>,
    verified: VerifiedModels,
    /// [`RECORDING_DEADLINE`]; shorter in tests.
    deadline: Duration,
}

/// Owns the dictation state. Cheap to clone; all clones share it.
#[derive(Clone)]
pub struct DictationService {
    shared: Arc<Shared>,
}

impl DictationService {
    pub fn new(
        models_dir: PathBuf,
        source: Arc<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        notify: Notify,
    ) -> Self {
        Self::with_deadline(models_dir, source, transcriber, notify, RECORDING_DEADLINE)
    }

    fn with_deadline(
        models_dir: PathBuf,
        source: Arc<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        notify: Notify,
        deadline: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                models_dir,
                source,
                transcriber,
                notify,
                core: Mutex::new(Core {
                    phase: Phase::Idle,
                    selected: ModelId::Base,
                    active: None,
                    reported_percent: 0,
                    recordings: 0,
                }),
                texts: Mutex::new(VecDeque::new()),
                verified: VerifiedModels::default(),
                deadline,
            }),
        }
    }

    pub fn models_dir(&self) -> &std::path::Path {
        &self.shared.models_dir
    }

    fn publish(&self) {
        (self.shared.notify)(Notice::Changed);
    }

    /// Applies `event`; `false` (and no change) when it is not allowed right now.
    fn apply(&self, core: &mut Core, event: Event) -> bool {
        match next(&core.phase, event) {
            Some(phase) => {
                core.phase = phase;
                self.publish();
                true
            }
            None => false,
        }
    }

    /// The state for `selected`, the models and whether a microphone is there. The device query
    /// runs here only, when the window asks, never on every change; the core is not locked
    /// meanwhile.
    pub fn status(&self, selected: ModelId) -> DictationStatus {
        let microphone = self.shared.source.has_input();
        let dir = &self.shared.models_dir;
        let models: Vec<ModelView> = model::MODELS
            .iter()
            .map(|spec| ModelView {
                id: spec.id,
                bytes: spec.bytes,
                installed: model::installed(dir, spec),
            })
            .collect();
        let installed = models
            .iter()
            .any(|view| view.id == selected && view.installed);
        let mut core = lock(&self.shared.core);
        core.selected = selected;
        DictationStatus {
            state: view(&core.phase, installed),
            models,
            source: model::SOURCE_HOST,
            microphone,
        }
    }

    /// Every transcript waiting, oldest first, each handed out once.
    pub fn take_texts(&self) -> Vec<DictationText> {
        lock(&self.shared.texts).drain(..).collect()
    }

    fn push_text(&self, text: DictationText) {
        {
            let mut texts = lock(&self.shared.texts);
            if texts.len() == MAX_WAITING_TEXTS {
                texts.pop_front();
            }
            texts.push_back(text);
        }
        (self.shared.notify)(Notice::TextReady);
    }

    /// Opens the microphone for `session`. Returns once it records (or failed to open).
    pub fn start(
        &self,
        session: SessionId,
        model: ModelId,
        language: Language,
    ) -> Result<(), DictationError> {
        let generation = {
            let mut core = lock(&self.shared.core);
            core.selected = model;
            if !model::installed(&self.shared.models_dir, model::spec(model)) {
                return Err(DictationError::NotInstalled);
            }
            if !self.apply(&mut core, Event::Record(session)) {
                return Err(DictationError::Busy);
            }
            core.recordings += 1;
            let generation = core.recordings;
            core.active = Some(Active {
                session,
                model,
                language,
                capture: None,
                stop_requested: false,
                generation,
            });
            generation
        };
        // Opening a device takes a moment; the lock is free meanwhile so a quick key release
        // (stop) is not blocked behind it.
        let notify = self.shared.notify.clone();
        let opened = self
            .shared
            .source
            .open(Box::new(move |level| notify(Notice::Level(level))));
        let mut core = lock(&self.shared.core);
        match opened {
            Ok(capture) => {
                let stop_now = match core.active.as_mut() {
                    Some(active) => {
                        active.capture = Some(capture);
                        active.stop_requested
                    }
                    None => false,
                };
                drop(core);
                if stop_now {
                    self.stop();
                } else {
                    self.arm_deadline(generation);
                }
                Ok(())
            }
            Err(error) => {
                core.active = None;
                let result = match &error {
                    CaptureError::NoMicrophone => DictationError::NoMicrophone,
                    CaptureError::PermissionDenied => DictationError::PermissionDenied,
                    CaptureError::Failed(_) => DictationError::Microphone,
                };
                self.apply(&mut core, Event::CaptureFailed(error));
                Err(result)
            }
        }
    }

    /// Stops recording `generation` once the deadline passed, should the window never send its
    /// stop (reloaded or crashed while the key was held).
    fn arm_deadline(&self, generation: u64) {
        let service = self.clone();
        let deadline = self.shared.deadline;
        let spawned = std::thread::Builder::new()
            .name("dictation-deadline".to_owned())
            .spawn(move || {
                std::thread::sleep(deadline);
                service.finish_recording(Some(generation));
            });
        if let Err(error) = spawned {
            eprintln!("hangar: dictation deadline not set: {error}");
        }
    }

    /// Closes the microphone and transcribes on a worker thread. Does nothing when not
    /// recording, so the window may call it on every key release, on blur and when it loads.
    pub fn stop(&self) {
        self.finish_recording(None);
    }

    /// [`Self::stop`]; with `only`, just when that recording still runs.
    fn finish_recording(&self, only: Option<u64>) {
        let mut core = lock(&self.shared.core);
        let Some(active) = core.active.as_mut() else {
            return;
        };
        if only.is_some_and(|generation| generation != active.generation) {
            return;
        }
        let Some(capture) = active.capture.take() else {
            active.stop_requested = true;
            return;
        };
        let (session, model, language) = (active.session, active.model, active.language);
        core.active = None;
        if !self.apply(&mut core, Event::Stop) {
            return;
        }
        drop(core);
        let recording = capture.finish();
        let service = self.clone();
        let spawned = std::thread::Builder::new()
            .name("dictation-transcribe".to_owned())
            .spawn(move || service.transcribe(session, model, language, recording));
        if spawned.is_err() {
            let mut core = lock(&self.shared.core);
            self.apply(
                &mut core,
                Event::TranscribeFailed("worker not started".to_owned()),
            );
        }
    }

    fn transcribe(
        &self,
        session: SessionId,
        model: ModelId,
        language: Language,
        recording: Recording,
    ) {
        let finish = |event: Event| {
            let mut core = lock(&self.shared.core);
            self.apply(&mut core, event);
        };
        if audio::looks_blocked(&recording) {
            finish(Event::Blocked);
            return;
        }
        let samples = audio::to_whisper(&recording);
        drop(recording);
        if !audio::has_speech(&samples) {
            finish(Event::Discard);
            return;
        }
        let spec = model::spec(model);
        let dir = &self.shared.models_dir;
        if let Err(error) = self.shared.verified.ensure(dir, spec) {
            // Not the pinned file any more: removed, so the window offers the download again
            // instead of failing on every recording.
            if error == ModelError::Mismatch {
                let _ = model::remove(dir, spec);
            }
            finish(Event::TranscribeFailed(error.to_string()));
            return;
        }
        let path = model::path(dir, spec);
        let result = self
            .shared
            .transcriber
            .transcribe(&path, &samples, language);
        drop(samples);
        match result {
            Ok(raw) => {
                let text = clean_transcript(&raw);
                if !text.is_empty() {
                    self.push_text(DictationText { session, text });
                }
                finish(Event::Transcribed);
            }
            Err(error) => finish(Event::TranscribeFailed(error.to_string())),
        }
    }

    /// Marks the download of `model` as started; `Busy` while something else runs.
    pub fn begin_install(&self, model: ModelId) -> Result<(), DictationError> {
        let mut core = lock(&self.shared.core);
        core.selected = model;
        core.reported_percent = 0;
        if self.apply(&mut core, Event::Install(model)) {
            Ok(())
        } else {
            Err(DictationError::Busy)
        }
    }

    /// Download progress; tells the window once per whole percent.
    pub fn install_progress(&self, received: u64, total: u64) {
        let mut core = lock(&self.shared.core);
        let percent = u32::try_from(received.saturating_mul(100) / total.max(1)).unwrap_or(100);
        if percent == core.reported_percent {
            return;
        }
        core.reported_percent = percent;
        self.apply(&mut core, Event::Progress(percent as f32 / 100.0));
    }

    pub fn end_install(&self, model: ModelId, result: &Result<PathBuf, ModelError>) {
        if result.is_ok() {
            // The download's hash just matched: no second read before the first load.
            self.shared
                .verified
                .mark(&self.shared.models_dir, model::spec(model));
        }
        let mut core = lock(&self.shared.core);
        let event = match result {
            Ok(_) => Event::Installed,
            Err(error) => Event::InstallFailed(error.to_string()),
        };
        self.apply(&mut core, event);
    }

    pub fn remove(&self, model: ModelId) -> Result<(), DictationError> {
        let mut core = lock(&self.shared.core);
        if !core.phase.settled() {
            return Err(DictationError::Busy);
        }
        model::remove(&self.shared.models_dir, model::spec(model))?;
        self.shared.verified.forget(model);
        self.apply(&mut core, Event::Removed);
        Ok(())
    }
}

/// Downloads `model` and reports progress through `service`. Consent happens in the window
/// before it calls this.
pub async fn install(service: &DictationService, model: ModelId) -> Result<(), DictationError> {
    service.begin_install(model)?;
    let spec = model::spec(model);
    let result = match model::client() {
        Ok(client) => {
            model::download(&client, spec, service.models_dir(), |received| {
                service.install_progress(received, spec.bytes);
            })
            .await
        }
        Err(error) => Err(error),
    };
    service.end_install(model, &result);
    result.map(|_| ()).map_err(DictationError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use zeroize::Zeroizing;

    #[test]
    fn transitions_follow_the_state_diagram() {
        let recording = Phase::Recording { session: 7 };
        assert_eq!(
            next(&Phase::Idle, Event::Record(7)),
            Some(recording.clone())
        );
        assert_eq!(
            next(&recording, Event::Stop),
            Some(Phase::Transcribing { session: 7 })
        );
        let transcribing = Phase::Transcribing { session: 7 };
        assert_eq!(next(&transcribing, Event::Transcribed), Some(Phase::Idle));
        assert_eq!(next(&transcribing, Event::Discard), Some(Phase::Idle));
        assert_eq!(next(&recording, Event::Discard), Some(Phase::Idle));
        assert_eq!(
            next(&transcribing, Event::Blocked),
            Some(Phase::PermissionDenied)
        );
        assert_eq!(
            next(&transcribing, Event::TranscribeFailed("x".into())),
            Some(Phase::Failed("x".into()))
        );
        assert_eq!(
            next(&recording, Event::CaptureFailed(CaptureError::NoMicrophone)),
            Some(Phase::NoMicrophone)
        );
        assert_eq!(
            next(
                &recording,
                Event::CaptureFailed(CaptureError::PermissionDenied)
            ),
            Some(Phase::PermissionDenied)
        );
    }

    #[test]
    fn busy_states_refuse_new_work() {
        let recording = Phase::Recording { session: 1 };
        let transcribing = Phase::Transcribing { session: 1 };
        let downloading = Phase::Downloading {
            model: ModelId::Base,
            progress: 0.5,
        };
        for busy in [&recording, &transcribing, &downloading] {
            assert_eq!(next(busy, Event::Record(2)), None, "{busy:?}");
            assert_eq!(next(busy, Event::Install(ModelId::Small)), None, "{busy:?}");
            assert_eq!(next(busy, Event::Removed), None, "{busy:?}");
        }
        assert_eq!(next(&Phase::Idle, Event::Stop), None);
        assert_eq!(next(&Phase::Idle, Event::Transcribed), None);
        assert_eq!(next(&Phase::Idle, Event::Progress(0.5)), None);
    }

    #[test]
    fn side_states_allow_a_retry() {
        for side in [
            Phase::NoMicrophone,
            Phase::PermissionDenied,
            Phase::Failed("x".into()),
        ] {
            assert_eq!(
                next(&side, Event::Record(3)),
                Some(Phase::Recording { session: 3 })
            );
            assert_eq!(
                next(&side, Event::Install(ModelId::Base)),
                Some(Phase::Downloading {
                    model: ModelId::Base,
                    progress: 0.0
                })
            );
            assert_eq!(next(&side, Event::Removed), Some(Phase::Idle));
        }
    }

    #[test]
    fn download_progress_is_clamped_and_ends_ready_or_failed() {
        let downloading = next(&Phase::Idle, Event::Install(ModelId::Small)).unwrap();
        assert_eq!(
            next(&downloading, Event::Progress(1.5)),
            Some(Phase::Downloading {
                model: ModelId::Small,
                progress: 1.0
            })
        );
        assert_eq!(next(&downloading, Event::Installed), Some(Phase::Idle));
        assert_eq!(
            next(&downloading, Event::InstallFailed("m".into())),
            Some(Phase::Failed("m".into()))
        );
    }

    #[test]
    fn idle_shows_ready_only_with_the_model() {
        assert_eq!(view(&Phase::Idle, true), DictationState::Ready);
        assert_eq!(view(&Phase::Idle, false), DictationState::NotInstalled);
        let json = serde_json::to_value(view(&Phase::Recording { session: 4 }, true)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "recording", "session": 4 })
        );
        let json = serde_json::to_value(view(
            &Phase::Downloading {
                model: ModelId::Small,
                progress: 0.25,
            },
            false,
        ))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "downloading", "model": "small", "progress": 0.25 })
        );
        assert_eq!(
            serde_json::to_value(DictationError::NotInstalled).unwrap(),
            "notInstalled"
        );
    }

    #[test]
    fn transcripts_become_one_clean_line() {
        assert_eq!(
            clean_transcript("  Führe bitte npm install aus.\n"),
            "Führe bitte npm install aus."
        );
        assert_eq!(clean_transcript("a\r\nb\tc"), "a b c");
        assert_eq!(clean_transcript("x\u{1b}[31my\u{7}"), "x[31my");
        assert_eq!(clean_transcript("ab\u{202E}cd\u{200B}e"), "abcde");
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript(" [MUSIC] hello "), "hello");
        // Brackets the speaker meant stay.
        assert_eq!(clean_transcript("use [x] here"), "use [x] here");
        assert_eq!(clean_transcript(""), "");
    }

    // ---------------------------------------------------------------------------------------
    // Service with a fake microphone and recognizer
    // ---------------------------------------------------------------------------------------

    struct FakeCapture(Vec<f32>);

    impl Capture for FakeCapture {
        fn finish(self: Box<Self>) -> Recording {
            Recording {
                samples: Zeroizing::new(self.0),
                rate: 16_000,
                channels: 1,
                callbacks: 10,
                elapsed: Duration::from_secs(1),
            }
        }
    }

    struct FakeSource(Result<Vec<f32>, CaptureError>);

    impl AudioSource for FakeSource {
        fn open(&self, level: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Capture>, CaptureError> {
            level(0.5);
            self.0
                .clone()
                .map(|samples| Box::new(FakeCapture(samples)) as Box<dyn Capture>)
        }

        fn has_input(&self) -> bool {
            self.0 != Err(CaptureError::NoMicrophone)
        }
    }

    struct FakeTranscriber(Result<String, whisper::TranscribeError>);

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &self,
            _model: &std::path::Path,
            _audio: &[f32],
            _language: Language,
        ) -> Result<String, whisper::TranscribeError> {
            self.0.clone()
        }
    }

    fn voice() -> Vec<f32> {
        (0..16_000).map(|i| 0.2 * (i as f32 * 0.09).sin()).collect()
    }

    fn make_service(
        name: &str,
        source: FakeSource,
        transcriber: FakeTranscriber,
        installed: bool,
    ) -> (DictationService, mpsc::Receiver<Notice>, PathBuf) {
        make_service_with(name, source, transcriber, installed, RECORDING_DEADLINE)
    }

    fn make_service_with(
        name: &str,
        source: FakeSource,
        transcriber: FakeTranscriber,
        installed: bool,
        deadline: Duration,
    ) -> (DictationService, mpsc::Receiver<Notice>, PathBuf) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join(".tmp")
            .join(format!("dictation-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let service = DictationService::with_deadline(
            dir.clone(),
            Arc::new(source),
            Arc::new(transcriber),
            Arc::new(move |notice| {
                let _ = lock(&tx).send(notice);
            }),
            deadline,
        );
        if installed {
            install_fake_model(&service, &dir);
        }
        (service, rx, dir)
    }

    /// A file of the pinned size, taken as checked: hashing 60 MB of zeros would only fail.
    fn install_fake_model(service: &DictationService, dir: &std::path::Path) {
        let spec = model::spec(ModelId::Base);
        let file = std::fs::File::create(model::path(dir, spec)).unwrap();
        file.set_len(spec.bytes).unwrap();
        drop(file);
        service.shared.verified.mark(dir, spec);
    }

    /// Waits for the state to settle back after a stop; returns the text waiting, if any.
    fn settle(
        service: &DictationService,
        rx: &mpsc::Receiver<Notice>,
    ) -> (Option<DictationText>, DictationState) {
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("no notice") {
                Notice::Changed => match service.status(ModelId::Base).state {
                    DictationState::Recording { .. } | DictationState::Transcribing { .. } => {}
                    state => {
                        let mut texts = service.take_texts();
                        assert!(texts.len() <= 1, "{texts:?}");
                        return (texts.pop(), state);
                    }
                },
                Notice::Level(_) | Notice::TextReady => {}
            }
        }
    }

    #[test]
    fn a_recording_ends_as_clean_text_for_its_session() {
        let (service, rx, dir) = make_service(
            "text",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok(" Hallo\n Welt ".into())),
            true,
        );
        assert_eq!(service.status(ModelId::Base).state, DictationState::Ready);
        service.start(5, ModelId::Base, Language::De).unwrap();
        assert_eq!(
            service.start(6, ModelId::Base, Language::De),
            Err(DictationError::Busy)
        );
        service.stop();
        let (text, state) = settle(&service, &rx);
        assert_eq!(
            text,
            Some(DictationText {
                session: 5,
                text: "Hallo Welt".into()
            })
        );
        assert_eq!(state, DictationState::Ready);
        // A second stop (key up after blur) changes nothing.
        service.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn silence_sends_nothing() {
        let (service, rx, dir) = make_service(
            "silence",
            FakeSource(Ok(vec![0.0005; 16_000])),
            FakeTranscriber(Ok("Untertitel".into())),
            true,
        );
        service.start(1, ModelId::Base, Language::Auto).unwrap();
        service.stop();
        let (text, state) = settle(&service, &rx);
        assert_eq!(text, None);
        assert_eq!(state, DictationState::Ready);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn digital_silence_means_the_microphone_is_blocked() {
        let (service, rx, dir) = make_service(
            "blocked",
            FakeSource(Ok(vec![0.0; 16_000])),
            FakeTranscriber(Ok("x".into())),
            true,
        );
        service.start(1, ModelId::Base, Language::Auto).unwrap();
        service.stop();
        assert_eq!(settle(&service, &rx).1, DictationState::PermissionDenied);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Released before the device delivered its first buffer: nothing happens, no notice.
    #[test]
    fn a_quick_tap_is_not_a_blocked_microphone() {
        struct TapCapture;
        impl Capture for TapCapture {
            fn finish(self: Box<Self>) -> Recording {
                Recording {
                    samples: Zeroizing::new(Vec::new()),
                    rate: 48_000,
                    channels: 2,
                    callbacks: 0,
                    elapsed: Duration::from_millis(15),
                }
            }
        }
        struct TapSource;
        impl AudioSource for TapSource {
            fn open(
                &self,
                _level: Box<dyn Fn(f32) + Send>,
            ) -> Result<Box<dyn Capture>, CaptureError> {
                Ok(Box::new(TapCapture))
            }

            fn has_input(&self) -> bool {
                true
            }
        }
        // The helper only provides the folder with an installed model here.
        let (_, _, dir) = make_service(
            "tap",
            FakeSource(Ok(Vec::new())),
            FakeTranscriber(Ok("x".into())),
            true,
        );
        let (notices, events) = mpsc::channel();
        let notices = Mutex::new(notices);
        let service = DictationService::new(
            dir.clone(),
            Arc::new(TapSource),
            Arc::new(FakeTranscriber(Ok("x".into()))),
            Arc::new(move |notice| {
                let _ = lock(&notices).send(notice);
            }),
        );
        service.start(1, ModelId::Base, Language::Auto).unwrap();
        service.stop();
        assert_eq!(settle(&service, &events), (None, DictationState::Ready));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_model_and_microphone_are_reported() {
        let (service, _rx, dir) = make_service(
            "nomodel",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("x".into())),
            false,
        );
        assert_eq!(
            service.status(ModelId::Base).state,
            DictationState::NotInstalled
        );
        assert_eq!(
            service.start(1, ModelId::Base, Language::Auto),
            Err(DictationError::NotInstalled)
        );
        let _ = std::fs::remove_dir_all(dir);

        let (service, _rx, dir) = make_service(
            "nomic",
            FakeSource(Err(CaptureError::NoMicrophone)),
            FakeTranscriber(Ok("x".into())),
            true,
        );
        assert_eq!(
            service.start(1, ModelId::Base, Language::Auto),
            Err(DictationError::NoMicrophone)
        );
        let status = service.status(ModelId::Base);
        assert_eq!(status.state, DictationState::NoMicrophone);
        assert!(!status.microphone);
        // The model can be removed again, and then the state says so.
        service.remove(ModelId::Base).unwrap();
        assert_eq!(
            service.status(ModelId::Base).state,
            DictationState::NotInstalled
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_recognition_shows_an_error() {
        let (service, rx, dir) = make_service(
            "fail",
            FakeSource(Ok(voice())),
            FakeTranscriber(Err(whisper::TranscribeError::Failed)),
            true,
        );
        service.start(1, ModelId::Base, Language::En).unwrap();
        service.stop();
        let (text, state) = settle(&service, &rx);
        assert_eq!(text, None);
        assert!(matches!(state, DictationState::Error { .. }), "{state:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_status_names_the_microphone() {
        let (service, _rx, dir) = make_service(
            "mic",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("x".into())),
            false,
        );
        let json = serde_json::to_value(service.status(ModelId::Base)).unwrap();
        assert_eq!(json["microphone"], serde_json::json!(true));
        assert_eq!(json["source"], serde_json::json!("huggingface.co"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The window that was reloaded while the key was held never sends its stop; the backend
    /// closes the microphone on its own and keeps what was said.
    #[test]
    fn a_recording_without_stop_ends_at_the_deadline() {
        let (service, rx, dir) = make_service_with(
            "deadline",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("up to here".into())),
            true,
            Duration::from_millis(50),
        );
        service.start(3, ModelId::Base, Language::De).unwrap();
        let (text, state) = settle(&service, &rx);
        assert_eq!(
            text.map(|t| (t.session, t.text)),
            Some((3, "up to here".into()))
        );
        assert_eq!(state, DictationState::Ready);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// An old deadline never ends a later recording.
    #[test]
    fn a_deadline_stops_only_its_own_recording() {
        let (service, rx, dir) = make_service(
            "generation",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("one".into())),
            true,
        );
        service.start(1, ModelId::Base, Language::De).unwrap();
        service.stop();
        let _ = settle(&service, &rx);
        service.start(1, ModelId::Base, Language::De).unwrap();
        service.finish_recording(Some(1));
        assert_eq!(
            service.status(ModelId::Base).state,
            DictationState::Recording { session: 1 }
        );
        service.finish_recording(Some(2));
        assert_eq!(settle(&service, &rx).1, DictationState::Ready);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn waiting_texts_are_handed_out_once_and_bounded() {
        let (service, rx, dir) = make_service(
            "texts",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("x".into())),
            false,
        );
        for i in 0..=MAX_WAITING_TEXTS {
            service.push_text(DictationText {
                session: i as SessionId,
                text: format!("t{i}"),
            });
        }
        assert_eq!(rx.try_iter().count(), MAX_WAITING_TEXTS + 1);
        let texts = service.take_texts();
        assert_eq!(texts.len(), MAX_WAITING_TEXTS);
        assert_eq!(texts[0].text, "t1", "the oldest went first");
        assert!(service.take_texts().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A model file that no longer matches its hash never reaches whisper; it is removed, so the
    /// window offers the download again.
    #[test]
    fn a_changed_model_file_is_refused_and_removed() {
        let (service, rx, dir) = make_service(
            "mismatch",
            FakeSource(Ok(voice())),
            FakeTranscriber(Ok("x".into())),
            true,
        );
        service.shared.verified.forget(ModelId::Base);
        service.start(1, ModelId::Base, Language::Auto).unwrap();
        service.stop();
        let (text, state) = settle(&service, &rx);
        assert_eq!(text, None);
        assert!(matches!(state, DictationState::Error { .. }), "{state:?}");
        assert!(!model::installed(&dir, model::spec(ModelId::Base)));
        assert_eq!(
            service.status(ModelId::Base).state,
            DictationState::Error {
                message: ModelError::Mismatch.to_string()
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
