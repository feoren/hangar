// Hangar's bar above an agent session's terminal: one quiet line in two groups, where the session
// works on the left (model, folder, worktree, branch, changed files) and how it goes on the right
// (context, output rate, cost, run time). Facts without data are left out (hangarBarFacts.ts);
// as the pane narrows, they give way in a fixed order through container queries (styles.css
// "Hangar bar"). It reads the same stores as the session row, so it asks the backend for nothing
// new.

import {
  compactTokens,
  contextHint,
  contextShare,
  formatOutputRate,
  formatShare,
  HIGH_SHARE,
  outputRateHint,
} from "./contextUsage";
import { barSegments, dropRank, type BarFacts, type BarSegment } from "./hangarBarFacts";
import { useT } from "./i18n";
import { BranchIcon, FolderIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { formatDuration, STATUS_TICK_MS } from "./statusLine";
import { useClock } from "./useClock";
import { useSessionContext, useSessionOutputRate } from "./useContextUsage";
import { useSessionCostText } from "./useCosts";

/** What the pane knows about its session; the bar reads the live numbers itself. */
export interface HangarBarProps {
  sessionId: number;
  /** Start of the process (epoch milliseconds). */
  createdAt: number;
  exited: boolean;
  model: { text: string; title: string } | null;
  folder: string | null;
  worktree: boolean;
  branch: string | null;
  changes: string | null;
  language: string;
}

export function HangarBar(props: HangarBarProps) {
  const context = useSessionContext(props.sessionId);
  const speed = useSessionOutputRate(props.sessionId);
  const cost = useSessionCostText(props.sessionId, props.language);
  const now = useClock(STATUS_TICK_MS);
  const runtime = props.exited ? null : formatDuration(now - props.createdAt, props.language);
  return (
    <HangarBarView
      facts={{
        model: props.model,
        folder: props.folder,
        worktree: props.worktree,
        branch: props.branch,
        changes: props.changes,
        context,
        speed,
        cost,
        runtime,
      }}
      language={props.language}
    />
  );
}

/** The bar for given facts; pure, so tests and previews draw the very same markup. */
export function HangarBarView({ facts, language }: { facts: BarFacts; language: string }) {
  const t = useT();
  const { where, how } = barSegments(facts);
  if (where.length === 0 && how.length === 0) return null;
  return (
    <div className="hangar-bar" role="group" aria-label={t("hangarBar.label")}>
      <div className="hangar-bar-inner">
        {where.length > 0 && (
          <span className="hangar-bar-group where">
            {where.map((segment) => (
              <Segment key={segment} segment={segment} facts={facts} language={language} />
            ))}
          </span>
        )}
        {how.length > 0 && (
          <span className="hangar-bar-group how">
            {how.map((segment) => (
              <Segment key={segment} segment={segment} facts={facts} language={language} />
            ))}
          </span>
        )}
      </div>
    </div>
  );
}

function Segment({
  segment,
  facts,
  language,
}: {
  segment: BarSegment;
  facts: BarFacts;
  language: string;
}) {
  const rank = dropRank(segment);
  const className = ["hangar-bar-seg", `seg-${segment}`, rank === 0 ? "" : `drop-${String(rank)}`]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={className} data-segment={segment}>
      <SegmentContent segment={segment} facts={facts} language={language} />
    </span>
  );
}

function SegmentContent({
  segment,
  facts,
  language,
}: {
  segment: BarSegment;
  facts: BarFacts;
  language: string;
}) {
  const t = useT();
  switch (segment) {
    case "model":
      return facts.model === null ? null : (
        <span className="hangar-bar-model" title={facts.model.title}>
          <span className="sr-only">{facts.model.title}</span>
          <span aria-hidden="true">{facts.model.text}</span>
        </span>
      );
    case "folder":
      return (
        <>
          <FolderIcon size={11} />
          <span className="sr-only">{t("rowElement.folder")}: </span>
          <span className="hangar-bar-text">{facts.folder}</span>
        </>
      );
    case "worktree":
      return <span className="hangar-bar-tag">{t("hangarBar.worktree")}</span>;
    case "branch": {
      const branch = revealInvisible(facts.branch ?? "");
      return (
        <>
          <BranchIcon size={11} />
          <span className="sr-only">{t("session.branch", { branch })}</span>
          <span className="hangar-bar-text" aria-hidden="true">
            {branch}
          </span>
        </>
      );
    }
    case "changes":
      return <span>{t("hangarBar.changed", { count: facts.changes ?? "" })}</span>;
    case "context":
      return facts.context === undefined ? null : (
        <ContextMeter context={facts.context} language={language} />
      );
    case "speed":
      return facts.speed === undefined ? null : (
        <span className={facts.speed.live ? undefined : "settled"}>
          <span className="sr-only">{outputRateHint(facts.speed, language, t)}</span>
          <span aria-hidden="true">
            {t("speed.value", { value: formatOutputRate(facts.speed.value, language) })}
          </span>
        </span>
      );
    case "cost":
      return (
        <>
          <span className="sr-only">{t("rowElement.cost")} </span>
          {facts.cost}
        </>
      );
    case "runtime":
      return (
        <>
          <span className="sr-only">{t("session.runtime", { duration: facts.runtime ?? "" })}</span>
          <span aria-hidden="true">{facts.runtime}</span>
        </>
      );
  }
}

/** A short still bar with the share after it; from 80 % both take the warning color. */
function ContextMeter({
  context,
  language,
}: {
  context: NonNullable<BarFacts["context"]>;
  language: string;
}) {
  const t = useT();
  const share = contextShare(context);
  const hint = contextHint(context, language, t);
  return (
    <span
      className={share >= HIGH_SHARE ? "hangar-bar-context high" : "hangar-bar-context"}
      title={hint}
    >
      <span className="sr-only">{hint}</span>
      <span className="hangar-bar-meter" aria-hidden="true">
        <span
          className="hangar-bar-meter-fill"
          style={{ width: `${String(Math.round(share * 100))}%` }}
        />
      </span>
      <span aria-hidden="true">{formatShare(share, language)}</span>
      <span className="hangar-bar-tokens" aria-hidden="true">
        {compactTokens(context.used)} / {compactTokens(context.window)}
      </span>
    </span>
  );
}
