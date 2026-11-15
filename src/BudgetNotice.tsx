import { useEffect, useState } from "react";

import { budgetWarning, formatUsd } from "./costs";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { isQuietTime, type QuietHours } from "./quietHours";
import { useCostReport } from "./useCosts";

const WARNED_KEY = "hangar.costs.warnedDay";
/** Quiet hours end on the wall clock; a minute's delay is fine for a budget notice. */
const CLOCK_MS = 60_000;

function readWarned(): string | null {
  try {
    return localStorage.getItem(WARNED_KEY);
  } catch {
    return null;
  }
}

function writeWarned(day: string): void {
  try {
    localStorage.setItem(WARNED_KEY, day);
  } catch {
    // Without storage the notice may show again after a restart; nothing else breaks.
  }
}

interface Props {
  language: string;
  budget: number | null;
  quietHours: QuietHours;
  onShowCosts: () => void;
}

/**
 * Once a day, when today's estimated cost reaches the daily budget: a notice with a way to the
 * cost overview. During quiet hours it waits until they end.
 */
export function BudgetNotice({ language, budget, quietHours, onShowCosts }: Props) {
  const t = useT();
  const report = useCostReport();
  const [now, setNow] = useState(() => Date.now());
  const [warned, setWarned] = useState(readWarned);
  const [shown, setShown] = useState<{ day: string; cost: number } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, CLOCK_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, []);

  const quiet = isQuietTime(quietHours, new Date(now));
  const due = budgetWarning({ report, budget, warnedDay: warned, quiet });
  // Adjusted during render: `due` turns null again right after, so this runs once per day.
  if (due !== null && report !== null) {
    setWarned(report.today);
    setShown({ day: report.today, cost: due });
  }
  useEffect(() => {
    if (warned !== null) writeWarned(warned);
  }, [warned]);

  if (shown === null || budget === null) return null;
  const close = () => {
    setShown(null);
  };
  return (
    <div className="link-notice budget-notice" role="status">
      <span>
        {t("costs.budgetReached", {
          cost: formatUsd(shown.cost, language),
          budget: formatUsd(budget, language),
        })}
      </span>
      <div className="link-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            close();
            onShowCosts();
          }}
        >
          {t("costs.show")}
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("error.dismiss")}
          title={t("error.dismiss")}
          onClick={close}
        >
          <CloseIcon />
        </button>
      </div>
    </div>
  );
}
