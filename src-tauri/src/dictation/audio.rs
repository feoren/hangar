//! Microphone capture and the audio math in front of whisper: downmix, resampling to 16 kHz and
//! the energy gate that drops silence. Audio stays in memory, is wiped once used and never logged.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::lock;

/// The sample rate whisper expects.
pub const WHISPER_RATE: u32 = 16_000;
/// Hard cap of one recording. Past it the microphone closes, what was said so far is kept.
pub const MAX_SECONDS: u32 = 60;

/// Why the microphone could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("No microphone found")]
    NoMicrophone,
    #[error("The operating system denied access to the microphone")]
    PermissionDenied,
    #[error("The microphone could not be opened: {0}")]
    Failed(String),
}

/// What a finished capture hands over: raw interleaved samples in the device's format.
pub struct Recording {
    pub samples: Zeroizing<Vec<f32>>,
    pub rate: u32,
    pub channels: u16,
    /// How many times the device delivered data; zero means the stream never ran.
    pub callbacks: u64,
}

/// A running capture. `finish` closes the microphone and returns what was recorded.
pub trait Capture: Send {
    fn finish(self: Box<Self>) -> Recording;
}

/// Opens the microphone. `level` receives the current input level (0 to 1) about ten times a
/// second while recording.
pub trait AudioSource: Send + Sync {
    fn open(&self, level: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Capture>, CaptureError>;
}

/// Averages the channels of each frame into one.
pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resamples mono audio from `from` Hz to `to` Hz. Downsampling first averages over the ratio
/// (a box low-pass against aliasing), then interpolates linearly. Plenty for speech recognition,
/// which only needs the band up to 8 kHz.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let filtered;
    let source: &[f32] = if ratio > 1.0 {
        filtered = box_filter(samples, ratio.round().max(1.0) as usize);
        &filtered
    } else {
        samples
    };
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    let last = source.len() - 1;
    (0..out_len)
        .map(|i| {
            let position = i as f64 * ratio;
            let index = position.floor() as usize;
            let next = (index + 1).min(last);
            let fraction = (position - index as f64) as f32;
            let a = source[index.min(last)];
            let b = source[next];
            a + (b - a) * fraction
        })
        .collect()
}

/// Centered moving average over `width` samples.
fn box_filter(samples: &[f32], width: usize) -> Vec<f32> {
    if width <= 1 {
        return samples.to_vec();
    }
    let half = width / 2;
    let mut prefix = Vec::with_capacity(samples.len() + 1);
    prefix.push(0.0f64);
    for sample in samples {
        let last = prefix.last().copied().unwrap_or(0.0);
        prefix.push(last + f64::from(*sample));
    }
    (0..samples.len())
        .map(|i| {
            let start = i.saturating_sub(half);
            let end = (i + width - half).min(samples.len());
            ((prefix[end] - prefix[start]) / (end - start) as f64) as f32
        })
        .collect()
}

/// Turns a recording into what whisper takes: mono, 16 kHz.
pub fn to_whisper(recording: &Recording) -> Zeroizing<Vec<f32>> {
    let mono = Zeroizing::new(downmix(&recording.samples, recording.channels));
    Zeroizing::new(resample(&mono, recording.rate, WHISPER_RATE))
}

/// Frame length of the energy gate: 20 ms at 16 kHz.
const FRAME: usize = 320;
/// A frame counts as voiced above this RMS (about -40 dBFS).
const VOICED_RMS: f32 = 0.01;
/// At least this many voiced frames (200 ms) before the audio goes to whisper. Whisper invents
/// text for silence ("Thank you.", "Untertitel von ..."), so silence never reaches it.
const MIN_VOICED_FRAMES: usize = 10;

/// Whether 16 kHz mono audio holds enough sound to be worth transcribing.
pub fn has_speech(samples: &[f32]) -> bool {
    samples
        .chunks(FRAME)
        .filter(|frame| rms(frame) > VOICED_RMS)
        .count()
        >= MIN_VOICED_FRAMES
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// The display level for a block of samples: RMS scaled so normal speech fills most of the meter.
pub fn display_level(samples: &[f32]) -> f32 {
    (rms(samples) * 6.0).clamp(0.0, 1.0)
}

/// Whether the device delivered nothing usable: no data at all, or only exact zeros (Windows
/// hands out digital silence when the microphone privacy switch blocks desktop apps
/// `[UNVERIFIED]`). Real microphones always carry some noise.
pub fn looks_blocked(recording: &Recording) -> bool {
    let enough = recording.rate as usize / 2 * usize::from(recording.channels.max(1));
    recording.callbacks == 0
        || (recording.samples.len() >= enough && recording.samples.iter().all(|s| *s == 0.0))
}

// ------------------------------------------------------------------------------------------
// cpal
// ------------------------------------------------------------------------------------------

/// The default input device through cpal (WASAPI, CoreAudio, ALSA).
pub struct CpalSource;

struct Shared {
    samples: Mutex<Zeroizing<Vec<f32>>>,
    cap: usize,
    callbacks: AtomicU32,
    level: AtomicU32,
    full: AtomicBool,
}

impl Shared {
    fn push(&self, block: &[f32]) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.level
            .store(display_level(block).to_bits(), Ordering::Relaxed);
        let mut samples = lock(&self.samples);
        let room = self.cap.saturating_sub(samples.len());
        let take = room.min(block.len());
        samples.extend_from_slice(&block[..take]);
        if take < block.len() {
            self.full.store(true, Ordering::Relaxed);
        }
    }
}

struct CpalCapture {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
    shared: Arc<Shared>,
    rate: u32,
    channels: u16,
}

impl Capture for CpalCapture {
    fn finish(self: Box<Self>) -> Recording {
        let CpalCapture {
            stop,
            thread,
            shared,
            rate,
            channels,
        } = *self;
        let _ = stop.send(());
        // The thread drops the stream (closes the microphone) before it ends.
        let _ = thread.join();
        let samples = std::mem::take(&mut *lock(&shared.samples));
        Recording {
            samples,
            rate,
            channels,
            callbacks: u64::from(shared.callbacks.load(Ordering::Relaxed)),
        }
    }
}

fn map_error(error: &cpal::Error) -> CaptureError {
    match error.kind() {
        cpal::ErrorKind::PermissionDenied => CaptureError::PermissionDenied,
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable => {
            CaptureError::NoMicrophone
        }
        _ => CaptureError::Failed(error.to_string()),
    }
}

/// Converts one block of any sample format to f32 and hands it to `shared`.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, cpal::Error> {
    use cpal::traits::DeviceTrait as _;
    use cpal::Sample as _;
    let stream_config = config.config();
    let on_error = |error: cpal::Error| {
        // The stream's own failure (device unplugged); the recording keeps what it has.
        eprintln!("hangar: dictation input stream: {error}");
    };
    macro_rules! stream {
        ($t:ty) => {{
            let shared = shared.clone();
            let mut block: Vec<f32> = Vec::new();
            device.build_input_stream::<$t, _, _>(
                stream_config,
                move |data: &[$t], _| {
                    block.clear();
                    block.extend(data.iter().map(|s| s.to_sample::<f32>()));
                    shared.push(&block);
                    block.iter_mut().for_each(|s| *s = 0.0);
                },
                on_error,
                None,
            )
        }};
    }
    match config.sample_format() {
        cpal::SampleFormat::F32 => stream!(f32),
        cpal::SampleFormat::I16 => stream!(i16),
        cpal::SampleFormat::U16 => stream!(u16),
        cpal::SampleFormat::I32 => stream!(i32),
        cpal::SampleFormat::U8 => stream!(u8),
        cpal::SampleFormat::I8 => stream!(i8),
        cpal::SampleFormat::F64 => stream!(f64),
        other => Err(cpal::Error::with_message(
            cpal::ErrorKind::UnsupportedConfig,
            format!("sample format {other:?}"),
        )),
    }
}

impl AudioSource for CpalSource {
    fn open(&self, level: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Capture>, CaptureError> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(Arc<Shared>, u32, u16), CaptureError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        // The stream lives on its own thread: cpal streams are not `Send` on every platform.
        let thread = std::thread::Builder::new()
            .name("dictation-capture".to_owned())
            .spawn(move || {
                use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
                let host = cpal::default_host();
                let Some(device) = host.default_input_device() else {
                    let _ = ready_tx.send(Err(CaptureError::NoMicrophone));
                    return;
                };
                let config = match device.default_input_config() {
                    Ok(config) => config,
                    Err(error) => {
                        let _ = ready_tx.send(Err(map_error(&error)));
                        return;
                    }
                };
                let rate = config.sample_rate();
                let channels = config.channels();
                let cap = (rate as usize)
                    .saturating_mul(MAX_SECONDS as usize)
                    .saturating_mul(usize::from(channels.max(1)));
                let shared = Arc::new(Shared {
                    samples: Mutex::new(Zeroizing::new(Vec::with_capacity(cap.min(1 << 22)))),
                    cap,
                    callbacks: AtomicU32::new(0),
                    level: AtomicU32::new(0),
                    full: AtomicBool::new(false),
                });
                let mut stream = match build_stream(&device, &config, shared.clone()) {
                    Ok(stream) => Some(stream),
                    Err(error) => {
                        let _ = ready_tx.send(Err(map_error(&error)));
                        return;
                    }
                };
                if let Some(Err(error)) = stream.as_ref().map(|s| s.play()) {
                    let _ = ready_tx.send(Err(map_error(&error)));
                    return;
                }
                let _ = ready_tx.send(Ok((shared.clone(), rate, channels)));
                // Until stop (or the window side hung up); meanwhile the level ten times a second.
                while let Err(mpsc::RecvTimeoutError::Timeout) =
                    stop_rx.recv_timeout(Duration::from_millis(100))
                {
                    if shared.full.load(Ordering::Relaxed) && stream.is_some() {
                        // The cap: the microphone closes now, the key may still be held.
                        stream = None;
                        level(0.0);
                    } else if stream.is_some() {
                        level(f32::from_bits(shared.level.load(Ordering::Relaxed)));
                    }
                }
                drop(stream);
            })
            .map_err(|error| CaptureError::Failed(error.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok((shared, rate, channels))) => Ok(Box::new(CpalCapture {
                stop: stop_tx,
                thread,
                shared,
                rate,
                channels,
            })),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::Failed("capture thread ended".to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let count = (rate as f32 * seconds) as usize;
        (0..count)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn downmix_averages_each_frame() {
        assert_eq!(
            downmix(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2),
            [0.5, 0.5, 0.0]
        );
        assert_eq!(downmix(&[0.25, 0.5], 1), [0.25, 0.5]);
        // A trailing partial frame is dropped rather than read past.
        assert_eq!(downmix(&[1.0, 1.0, 1.0], 2), [1.0]);
    }

    #[test]
    fn resample_keeps_length_and_pitch() {
        let tone = sine(440.0, 48_000, 1.0, 0.5);
        let out = resample(&tone, 48_000, WHISPER_RATE);
        assert_eq!(out.len(), 16_000);
        // 440 Hz crosses zero 880 times a second, before and after.
        let crossings = zero_crossings(&out);
        assert!((870..=890).contains(&crossings), "{crossings}");

        let cd = sine(440.0, 44_100, 1.0, 0.5);
        assert_eq!(resample(&cd, 44_100, WHISPER_RATE).len(), 16_000);
        let low = sine(440.0, 8_000, 1.0, 0.5);
        assert_eq!(resample(&low, 8_000, WHISPER_RATE).len(), 16_000);
        assert_eq!(
            resample(&[0.1, 0.2], WHISPER_RATE, WHISPER_RATE),
            [0.1, 0.2]
        );
        assert!(resample(&[], 48_000, WHISPER_RATE).is_empty());
    }

    #[test]
    fn resample_damps_tones_above_the_new_band() {
        // 15 kHz cannot exist at 16 kHz; the box filter keeps it from folding back loudly.
        let high = sine(15_000.0, 48_000, 0.5, 0.5);
        let out = resample(&high, 48_000, WHISPER_RATE);
        assert!(rms(&out) < 0.1, "{}", rms(&out));
        let speech_band = sine(1_000.0, 48_000, 0.5, 0.5);
        assert!(rms(&resample(&speech_band, 48_000, WHISPER_RATE)) > 0.3);
    }

    #[test]
    fn to_whisper_turns_stereo_48k_into_mono_16k() {
        let mono = sine(300.0, 48_000, 0.5, 0.4);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        let recording = Recording {
            samples: Zeroizing::new(stereo),
            rate: 48_000,
            channels: 2,
            callbacks: 1,
        };
        assert_eq!(to_whisper(&recording).len(), 8_000);
    }

    #[test]
    fn energy_gate_drops_silence_noise_and_clicks() {
        assert!(!has_speech(&[]));
        assert!(!has_speech(&vec![0.0; 16_000]));
        // Faint room noise, well under the threshold.
        assert!(!has_speech(&sine(120.0, 16_000, 2.0, 0.003)));
        // A click of 100 ms is not speech.
        let mut click = vec![0.0; 16_000];
        click[..1_600].copy_from_slice(&sine(500.0, 16_000, 0.1, 0.5));
        assert!(!has_speech(&click));
        // Half a second of voice level passes.
        let mut voice = vec![0.0; 16_000];
        voice[4_000..12_000].copy_from_slice(&sine(220.0, 16_000, 0.5, 0.1));
        assert!(has_speech(&voice));
    }

    #[test]
    fn blocked_microphone_is_recognized() {
        let recording = |samples: Vec<f32>, callbacks| Recording {
            samples: Zeroizing::new(samples),
            rate: 16_000,
            channels: 1,
            callbacks,
        };
        assert!(looks_blocked(&recording(Vec::new(), 0)));
        assert!(looks_blocked(&recording(vec![0.0; 16_000], 50)));
        assert!(!looks_blocked(&recording(vec![0.0001; 16_000], 50)));
        // Too short to judge: a quick tap of the key.
        assert!(!looks_blocked(&recording(vec![0.0; 100], 1)));
    }

    #[test]
    fn display_level_is_clamped() {
        assert_eq!(display_level(&[]), 0.0);
        assert_eq!(display_level(&[1.0, -1.0]), 1.0);
        assert!(display_level(&sine(200.0, 16_000, 0.1, 0.05)) > 0.1);
    }
}
