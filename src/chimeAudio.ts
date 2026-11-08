// The chime itself, synthesized with Web Audio, so no audio file ships. Waiting: two soft, rising
// sine notes (E5, A5) with a faint octave above for warmth. Failure: two lower, falling notes
// (A4, E4) through a gentle low-pass, darker but just as calm. Every note fades in over a few
// milliseconds and decays exponentially, so nothing clicks.
//
// The AudioContext is created on first use only. Every failure (no Web Audio, autoplay refused)
// is swallowed: the sound is an extra next to the sidebar, the taskbar and the notification.

import { chimeGain, type ChimeKind } from "./chime";

interface Note {
  frequency: number;
  start: number;
  length: number;
}

const NOTES: Record<ChimeKind, readonly Note[]> = {
  waiting: [
    { frequency: 659.25, start: 0, length: 0.42 },
    { frequency: 880, start: 0.12, length: 0.6 },
  ],
  failed: [
    { frequency: 440, start: 0, length: 0.42 },
    { frequency: 329.63, start: 0.15, length: 0.7 },
  ],
};

const ATTACK_S = 0.012;
/** Exponential ramps cannot reach zero; this is inaudible. */
const SILENT = 0.0001;

let context: AudioContext | null = null;

function audioContext(): AudioContext | null {
  if (context !== null) return context;
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  context = new window.AudioContext();
  return context;
}

/**
 * A resume that takes longer than this belongs to a moment that has passed: the chime is
 * dropped instead of sounding late (or together with the next one).
 */
const RESUME_LIMIT_MS = 500;

/** Plays one chime at the volume setting (0 to 100). Never throws. */
export function playChime(kind: ChimeKind, volume: number): void {
  const peak = chimeGain(volume);
  if (peak <= 0) return;
  try {
    // Before the first click or key press in the window, the engine keeps audio suspended and a
    // resume only settles once a later one succeeds, so the sound would come minutes too late.
    if (!navigator.userActivation.hasBeenActive) return;
    const audio = audioContext();
    if (audio === null) return;
    const start = () => {
      schedule(audio, kind, peak);
    };
    if (audio.state === "suspended") {
      const asked = performance.now();
      audio.resume().then(
        () => {
          if (performance.now() - asked <= RESUME_LIMIT_MS) start();
        },
        () => undefined,
      );
    } else start();
  } catch {
    // No sound then; everything else still signals the session.
  }
}

function schedule(audio: AudioContext, kind: ChimeKind, peak: number): void {
  const now = audio.currentTime + 0.02;
  const output = audio.createGain();
  output.gain.value = 1;
  let last: AudioNode = output;
  if (kind === "failed") {
    const filter = audio.createBiquadFilter();
    filter.type = "lowpass";
    filter.frequency.value = 1400;
    output.connect(filter);
    last = filter;
  }
  last.connect(audio.destination);
  let end = now;
  for (const note of NOTES[kind]) {
    const at = now + note.start;
    const stop = at + note.length;
    end = Math.max(end, stop);
    tone(audio, output, note.frequency, at, stop, peak);
    // A faint octave above gives the sine some warmth without sharpness.
    tone(audio, output, note.frequency * 2, at, stop, peak * 0.12);
  }
  window.setTimeout(
    () => {
      last.disconnect();
      if (last !== output) output.disconnect();
    },
    (end - audio.currentTime + 0.1) * 1000,
  );
}

function tone(
  audio: AudioContext,
  output: AudioNode,
  frequency: number,
  at: number,
  stop: number,
  peak: number,
): void {
  const oscillator = audio.createOscillator();
  oscillator.type = "sine";
  oscillator.frequency.value = frequency;
  const gain = audio.createGain();
  gain.gain.setValueAtTime(SILENT, at);
  gain.gain.exponentialRampToValueAtTime(peak, at + ATTACK_S);
  gain.gain.exponentialRampToValueAtTime(SILENT, stop);
  oscillator.connect(gain);
  gain.connect(output);
  oscillator.start(at);
  oscillator.stop(stop + 0.02);
  oscillator.onended = () => {
    gain.disconnect();
  };
}
