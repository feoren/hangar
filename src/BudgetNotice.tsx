import { useEffect, useState } from "react";

import { budgetWarning, formatUsd, type BudgetLevel, type BudgetWarned } from "./costs";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { isQuietTime, type QuietHours } from "./quietHours";
import { useCostReport } from "./useCosts";

/** Quiet hours end on the wall clock; a minute's delay is fine for a budget notice. */
const CLOCK_MS = 60_000;

interface Props {
  language: string;
  /** `null` also while the stored preferences are not read yet. */
  budget: number | null;
  /** The early warning's share of the budget in percent; `null`: off. */
  earlyPercent: number | null;
  /** The day and level the notice last showed, as the backend keeps it (`mainStore.ts`). */
  warned: BudgetWarned | null;
  /** A notice showed; the caller stores its day and level. */
  onWarned: (warned: BudgetWarned) => void;
  quietHours: QuietHours;
  onShowCosts: () => void;
}

/**
 * Once a day per level: when today's estimated cost reaches the early share of the daily budget,
 * and when it reaches the budget, a notice with a way to the cost overview. During quiet hours it
 * waits until they end.
 */
export function BudgetNotice({
  language,
  budget,
  earlyPercent,
  warned: storedWarned,
  onWarned,
  quietHours,
  onShowCosts,
}: Props) {
  const t = useT();
  const report = useCostReport();
  const [now, setNow] = useState(() => Date.now());
  /** The level this notice showed in this run; it counts until the caller's value caught up. */
  const [shownWarned, setShownWarned] = useState<BudgetWarned | null>(null);
  const warned = shownWarned ?? storedWarned;
  const [shown, setShown] = useState<{ level: BudgetLevel; cost: number } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, CLOCK_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, []);

  const quiet = isQuietTime(quietHours, new Date(now));
  const due = budgetWarning({ report, budget, earlyPercent, warned, quiet });
  // Adjusted during render: `due` turns null again right after, so this runs once per level.
  if (due !== null && report !== null) {
    setShownWarned({ day: report.today, level: due.level });
    setShown(due);
  }
  useEffect(() => {
    if (shownWarned !== null) onWarned(shownWarned);
  }, [shownWarned, onWarned]);

  if (shown === null || budget === null) return null;
  const close = () => {
    setShown(null);
  };
  const cost = formatUsd(shown.cost, language);
  const whole = formatUsd(budget, language);
  return (
    <div className="link-notice budget-notice" role="status">
      <span>
        {shown.level === "early"
          ? t("costs.budgetEarly", {
              cost,
              budget: whole,
              // Never below the chosen share: rounding may put 80 % of a budget at 79.99 %.
              percent: Math.max(earlyPercent ?? 0, Math.floor((shown.cost / budget) * 100)),
            })
          : t("costs.budgetReached", { cost, budget: whole })}
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
