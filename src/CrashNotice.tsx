import { useEffect } from "react";

import { crashNoticeLines, hasProblems, type CrashSummary } from "./crashRestore";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";

/** Without a session to name, the notice goes by itself after this long. */
export const CRASH_NOTICE_MS = 15_000;

interface Props {
  summary: CrashSummary;
  language: string;
  onClose: () => void;
}

/**
 * After the restore of a run that did not quit cleanly: how many sessions came back and which did
 * not come back whole. Names sessions until closed; otherwise it goes by itself.
 */
export function CrashNotice({ summary, language, onClose }: Props) {
  const t = useT();
  const problems = hasProblems(summary);
  useEffect(() => {
    if (problems) return;
    const timer = window.setTimeout(onClose, CRASH_NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [problems, onClose]);

  const [headline, ...details] = crashNoticeLines(summary, t, language);
  return (
    <div className="link-notice crash-notice" role="status">
      <span>
        {headline}
        {details.map((line) => (
          <span key={line} className="crash-notice-detail">
            {line}
          </span>
        ))}
      </span>
      <div className="link-actions">
        <button
          type="button"
          className="icon-button"
          aria-label={t("error.dismiss")}
          title={t("error.dismiss")}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
    </div>
  );
}
