import { compactTokens } from "./contextUsage";
import { costText, dayLabel, formatUsd, todayCost, type CostAmount } from "./costs";
import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { useCostReport } from "./useCosts";

interface Props {
  language: string;
  /** Dollars per day from Settings; `null`: no budget. */
  budget: number | null;
  onClose: () => void;
}

/** "1.2M tokens · ~$4.20", or the tokens alone when nothing has a price. */
function amountText(amount: CostAmount, language: string, tokens: (count: string) => string) {
  const count = tokens(compactTokens(amount.total));
  const cost = costText(amount, language);
  return cost === null ? count : `${count} · ${cost}`;
}

/**
 * Tokens and estimated cost: today per session, then the last seven days as bars. Only sessions
 * whose program keeps readable usage (Claude Code) appear; nothing else is guessed.
 */
export function CostDialog({ language, budget, onClose }: Props) {
  const t = useT();
  const report = useCostReport();
  const today = report === null ? undefined : todayCost(report);
  const days = report?.days ?? [];
  const empty = days.every((day) => day.total === 0) && (report?.sessions.size ?? 0) === 0;
  const tokens = (count: string) => t("costs.tokens", { count });
  const widest = Math.max(...days.map((day) => day.costUsd ?? 0), 0);
  const most = Math.max(...days.map((day) => day.total), 0);
  // Bars by cost where every day has one, by tokens otherwise.
  const byCost = widest > 0 && days.every((day) => day.total === 0 || day.costUsd !== null);
  const share = (day: CostAmount) =>
    byCost ? (day.costUsd ?? 0) / widest : most > 0 ? day.total / most : 0;

  return (
    <Dialog labelledBy="costs-title" className="cost-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="costs-title">{t("costs.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
          data-autofocus
        >
          <CloseIcon />
        </button>
      </div>

      {report === null || empty ? (
        <p className="muted cost-empty">{t("costs.empty")}</p>
      ) : (
        <>
          <section className="cost-section" aria-labelledby="costs-today">
            <h3 id="costs-today" className="settings-subhead cost-head">
              <span>{t("costs.today")}</span>
              {today !== undefined && today.total > 0 && (
                <span className="cost-sum">{amountText(today, language, tokens)}</span>
              )}
            </h3>
            {budget !== null && today?.costUsd != null && (
              <p
                className={
                  today.costUsd >= budget ? "cost-budget cost-budget-over" : "cost-budget muted"
                }
              >
                {t("costs.budgetUse", {
                  cost: formatUsd(today.costUsd, language),
                  budget: formatUsd(budget, language),
                })}
              </p>
            )}
            {today === undefined || today.sessions.length === 0 ? (
              <p className="muted">{t("costs.noneToday")}</p>
            ) : (
              <ul className="cost-list">
                {today.sessions.map((session, index) => (
                  <li key={`${session.title}-${String(index)}`} className="cost-row">
                    <span className="cost-name">
                      {session.title}
                      {session.program !== "" && session.program !== session.title && (
                        <span className="muted"> · {session.program}</span>
                      )}
                      {session.sessionId === null && (
                        <span className="muted"> · {t("costs.closed")}</span>
                      )}
                    </span>
                    <span className="cost-value">{amountText(session, language, tokens)}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="cost-section" aria-labelledby="costs-week">
            <h3 id="costs-week" className="settings-subhead">
              {t("costs.week")}
            </h3>
            <ul className="cost-days">
              {days.map((day) => (
                <li key={day.day} className="cost-day">
                  <span className="cost-day-name">
                    {dayLabel(day.day, report.today, language, t)}
                  </span>
                  <span className="cost-bar" aria-hidden="true">
                    <span style={{ width: `${String(Math.round(share(day) * 100))}%` }} />
                  </span>
                  <span className="cost-value">
                    {day.total === 0 ? "·" : amountText(day, language, tokens)}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        </>
      )}

      <p className="muted cost-note">
        {report !== null && report.priceSource !== ""
          ? t("costs.note", { source: report.priceSource, date: report.priceDate })
          : t("costs.noteNoPrices")}
      </p>
    </Dialog>
  );
}
