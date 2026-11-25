// The round red count of running sub-agents on a session's sidebar row and its rail button
// (styles.css, "Sub-agent count"). Nothing shows while none runs or once the session has ended.
// The visible dot always lies outside the flow (on the badge's corner, else the row's corner), so
// it never takes width from the row; the spoken text stays at the end of the row's button.

import { useT } from "./i18n";
import { badgeLabel, badgeText } from "./subAgentCounts";
import { useSubAgentCount } from "./useSubAgentCounts";
import { useWatchedRunning } from "./useSubAgents";

/**
 * Running sub-agents of one session: from its watch while the tree or view follows it (the same
 * snapshot the tree draws), else from the backend's count of every session. An ended session
 * shows none, even while its last sub-agents still count as running for up to a minute.
 */
export function useRunningSubAgents(
  sessionId: number,
  cliSessionId: string | null,
  exited: boolean,
): number {
  const counted = useSubAgentCount(sessionId);
  const watched = useWatchedRunning(sessionId, cliSessionId);
  return exited ? 0 : (watched ?? counted);
}

/** Where a visible dot sits: on the badge, at the row's top right corner, on the rail's badge. */
export type CountPlace = "badge" | "corner" | "rail";

const PLACE_CLASS: Record<CountPlace, string> = {
  badge: "subagent-count badge-count",
  corner: "subagent-count row-corner",
  rail: "subagent-count rail-subagents",
};

/** The visible dot, decorative: the label below (or the rail's description) says it in words. */
export function SubAgentCountMark({ running, place }: { running: number; place: CountPlace }) {
  const t = useT();
  if (running <= 0) return null;
  return (
    <span className={PLACE_CLASS[place]} title={badgeLabel(running, t)} aria-hidden="true">
      {badgeText(running)}
    </span>
  );
}

/**
 * The spoken text, for screen readers only. `id` names it for a button whose own `aria-label`
 * would hide it (the rail's, via `aria-describedby`).
 */
export function SubAgentCountLabel({ running, id }: { running: number; id?: string }) {
  const t = useT();
  if (running <= 0) return null;
  return (
    <span className="sr-only" id={id}>
      {badgeLabel(running, t)}
    </span>
  );
}

/** The rail button's dot and its text. */
export function RailSubAgentCount({
  sessionId,
  cliSessionId,
  exited,
}: {
  sessionId: number;
  cliSessionId: string | null;
  exited: boolean;
}) {
  const running = useRunningSubAgents(sessionId, cliSessionId, exited);
  return (
    <>
      <SubAgentCountMark running={running} place="rail" />
      <SubAgentCountLabel running={running} id={railCountId(sessionId)} />
    </>
  );
}

/** The id of a rail button's count text, for its `aria-describedby`. */
export function railCountId(sessionId: number): string {
  return `rail-subagents-${String(sessionId)}`;
}
