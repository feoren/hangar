import { useEffect, useId, useState } from "react";

import { useT } from "./i18n";
import type { UsageConsent } from "./usageStats";

/** The question waits this long after the start, so the window is calm and ready first. */
export const USAGE_NOTICE_DELAY_MS = 20_000;

interface Props {
  /** The stored answer; `undefined` while it is being read. */
  consent: UsageConsent | undefined;
  /** The person's answer; the caller stores it, and the question never comes back. */
  onAnswer: (enabled: boolean) => void;
  /** "What is sent": the public list of every field, in the browser. */
  onOpenInfo: () => void;
  delayMs?: number;
}

/**
 * The one question about the anonymous usage statistics, as a quiet notice in the corner: only
 * while it was never answered, a little after the start, never as a dialog in the way. Nothing is
 * sent unless the person says yes.
 */
export function UsageStatsNotice({
  consent,
  onAnswer,
  onOpenInfo,
  delayMs = USAGE_NOTICE_DELAY_MS,
}: Props) {
  const t = useT();
  const titleId = useId();
  const [due, setDue] = useState(false);
  const [answered, setAnswered] = useState(false);
  const open = consent === null;

  useEffect(() => {
    if (!open) return;
    const timer = window.setTimeout(() => {
      setDue(true);
    }, delayMs);
    return () => {
      window.clearTimeout(timer);
    };
  }, [open, delayMs]);

  if (!open || !due || answered) return null;
  const answer = (enabled: boolean) => {
    setAnswered(true);
    onAnswer(enabled);
  };
  return (
    // Read out politely like every other card of the stack: each card names its own role (some
    // are alerts), so the stack itself carries no live region.
    <div className="link-notice usage-notice" role="status" aria-labelledby={titleId}>
      <strong id={titleId} className="usage-notice-title">
        {t("usage.title")}
      </strong>
      <span>{t("usage.text")}</span>
      <div className="link-actions">
        <button type="button" className="link-button usage-notice-info" onClick={onOpenInfo}>
          {t("usage.whatIsSent")}
        </button>
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            answer(false);
          }}
        >
          {t("usage.noThanks")}
        </button>
        {/* Both answers look alike: the question asks, it does not push. */}
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            answer(true);
          }}
        >
          {t("usage.send")}
        </button>
      </div>
    </div>
  );
}
