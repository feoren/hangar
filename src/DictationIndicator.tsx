// The recording dot on the pane the dictation goes to: red dot and input level while the
// microphone is open, "Transcribing" afterwards, or a short notice when it could not start.

import { useT, type MessageKey } from "./i18n";
import { useDictationLevel, type DictationProblem } from "./useDictation";

export type IndicatorMode = "recording" | "transcribing" | DictationProblem;

const PROBLEM_TEXT: Record<DictationProblem, MessageKey> = {
  noMicrophone: "dictation.noMicrophone",
  permissionDenied: "dictation.permissionDenied",
  microphone: "dictation.microphoneFailed",
  other: "dictation.failed",
};

function Level() {
  const level = useDictationLevel();
  return (
    <span className="dictation-level" aria-hidden="true">
      <span style={{ transform: `scaleX(${String(Math.max(0.04, level))})` }} />
    </span>
  );
}

export function DictationIndicator({ mode }: { mode: IndicatorMode }) {
  const t = useT();
  if (mode === "recording") {
    return (
      <div className="dictation-indicator recording" role="status">
        <span className="dictation-dot" aria-hidden="true" />
        <span>{t("dictation.recording")}</span>
        <Level />
      </div>
    );
  }
  if (mode === "transcribing") {
    return (
      <div className="dictation-indicator" role="status">
        <span className="dictation-spinner" aria-hidden="true" />
        <span>{t("dictation.transcribing")}</span>
      </div>
    );
  }
  return (
    <div className="dictation-indicator problem" role="alert">
      {t(PROBLEM_TEXT[mode])}
    </div>
  );
}
