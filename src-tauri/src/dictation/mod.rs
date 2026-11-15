//! Local dictation: hold a key, speak, the text lands in the session's prompt. Recognition runs
//! on this machine (whisper.cpp); the only network use is the model download the user agreed to.
//! Audio stays in memory, is capped at 60 s, wiped after use and never logged or stored; the
//! transcript goes to the window once and is not kept either.
//!
//! The window asks for the text to go into the session it named on key down; it pastes it there
//! through its usual paste path (bracketed paste, control characters stripped), without Enter.

pub mod audio;
pub mod model;
pub mod whisper;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Serialize, Serializer};

use crate::lock;
use crate::session::SessionId;
use audio::{AudioSource, Capture, CaptureError, Recording};
use model::{ModelError, ModelId};
use whisper::{Language, Transcriber};

/// Events to the main window.
pub const STATUS_EVENT: &str = "dictation-status";
pub const LEVEL_EVENT: &str = "dictation-level";
pub const TEXT_EVENT: &str = "dictation-text";

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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationText {
    pub session: SessionId,
    pub text: String,
}

/// What the service tells the window.
pub enum Notice {
    Status(DictationStatus),
    Level(f32),
    Text(DictationText),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DictationError {
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
}

struct Core {
    phase: Phase,
    selected: ModelId,
    active: Option<Active>,
    /// Last download progress reported, in whole percent.
    reported_percent: u32,
}

struct Shared {
    models_dir: PathBuf,
    source: Arc<dyn AudioSource>,
    transcriber: Arc<dyn Transcriber>,
    notify: Notify,
    core: Mutex<Core>,
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
                }),
            }),
        }
    }

    pub fn models_dir(&self) -> &std::path::Path {
        &self.shared.models_dir
    }

    fn snapshot(&self, core: &Core) -> DictationStatus {
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
            .any(|view| view.id == core.selected && view.installed);
        DictationStatus {
            state: view(&core.phase, installed),
            models,
            source: model::SOURCE_HOST,
        }
    }

    fn publish(&self, core: &Core) {
        (self.shared.notify)(Notice::Status(self.snapshot(core)));
    }

    /// Applies `event`; `false` (and no change) when it is not allowed right now.
    fn apply(&self, core: &mut Core, event: Event) -> bool {
        match next(&core.phase, event) {
            Some(phase) => {
                core.phase = phase;
                self.publish(core);
                true
            }
            None => false,
        }
    }

    pub fn status(&self, selected: ModelId) -> DictationStatus {
        let mut core = lock(&self.shared.core);
        core.selected = selected;
        self.snapshot(&core)
    }

    /// Opens the microphone for `session`. Returns once it records (or failed to open).
    pub fn start(
        &self,
        session: SessionId,
        model: ModelId,
        language: Language,
    ) -> Result<(), DictationError> {
        {
            let mut core = lock(&self.shared.core);
            core.selected = model;
            if !model::installed(&self.shared.models_dir, model::spec(model)) {
                return Err(DictationError::NotInstalled);
            }
            if !self.apply(&mut core, Event::Record(session)) {
                return Err(DictationError::Busy);
            }
            core.active = Some(Active {
                session,
                model,
                language,
                capture: None,
                stop_requested: false,
            });
        }
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

    /// Closes the microphone and transcribes on a worker thread. Does nothing when not
    /// recording, so the window may call it on every key release and on blur.
    pub fn stop(&self) {
        let mut core = lock(&self.shared.core);
        let Some(active) = core.active.as_mut() else {
            return;
        };
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
        let path = model::path(&self.shared.models_dir, model::spec(model));
        let result = self
            .shared
            .transcriber
            .transcribe(&path, &samples, language);
        drop(samples);
        match result {
            Ok(raw) => {
                let text = clean_transcript(&raw);
                if !text.is_empty() {
                    (self.shared.notify)(Notice::Text(DictationText { session, text }));
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

    pub fn end_install(&self, result: &Result<PathBuf, ModelError>) {
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
    service.end_install(&result);
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
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join(".tmp")
            .join(format!("dictation-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if installed {
            let spec = model::spec(ModelId::Base);
            let file = std::fs::File::create(model::path(&dir, spec)).unwrap();
            file.set_len(spec.bytes).unwrap();
        }
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let service = DictationService::new(
            dir.clone(),
            Arc::new(source),
            Arc::new(transcriber),
            Arc::new(move |notice| {
                let _ = lock(&tx).send(notice);
            }),
        );
        (service, rx, dir)
    }

    /// Waits for the state to settle back after a stop; returns the text sent, if any.
    fn settle(rx: &mpsc::Receiver<Notice>) -> (Option<DictationText>, DictationState) {
        let mut text = None;
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("no notice") {
                Notice::Text(t) => text = Some(t),
                Notice::Status(status) => match status.state {
                    DictationState::Recording { .. } | DictationState::Transcribing { .. } => {}
                    state => return (text, state),
                },
                Notice::Level(_) => {}
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
        let (text, state) = settle(&rx);
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
        let (text, state) = settle(&rx);
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
        assert_eq!(settle(&rx).1, DictationState::PermissionDenied);
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
        assert_eq!(
            service.status(ModelId::Base).state,
            DictationState::NoMicrophone
        );
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
        let (text, state) = settle(&rx);
        assert_eq!(text, None);
        assert!(matches!(state, DictationState::Error { .. }), "{state:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
