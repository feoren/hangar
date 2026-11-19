// The tree flow's markup (treeFlow.ts): a lit trunk per group or sub-agent branch, a lit elbow
// per row and the rate on a working session's row. Each part renders nothing while the flow is
// off. All parts stay in place while their row is not live, faded out, so the flow fades in and
// out instead of popping; the animation itself is pure CSS (styles.css, "Tree flow").

import { useT } from "./i18n";
import { flowEnd, rateParts, type FlowLevel } from "./treeFlow";
import { useFlowLevel, useFlowRate, useHighestLevel, useTreeFlow } from "./treeFlowStore";

/**
 * The row that ended each trunk before, per group or branch key, so a trunk whose last live row
 * stops fades out at its length (`flowEnd`). Written during render with the same result for the
 * same input, so a repeated render changes nothing.
 */
const ends = new Map<string, number | string>();

/** The id of the row whose elbow ends the lit trunk of `key`; `null`: no trunk. */
export function trunkEnd<T extends number | string>(
  key: string,
  rows: readonly T[],
  live: readonly T[],
): T | null {
  const previous = ends.get(key);
  const known = rows.find((row) => row === previous) ?? null;
  const end = flowEnd(known, rows, live);
  if (end === null) ends.delete(key);
  else ends.set(key, end);
  return end;
}

/**
 * Which rows of a group or branch are lit and which one ends its trunk: the rows that `works`
 * (a session working, a sub-agent running), in their order, and `trunkEnd` of them.
 */
export function litRows<T, K extends number | string>(
  key: string,
  rows: readonly T[],
  id: (row: T) => K,
  works: (row: T) => boolean,
): { live: K[]; end: K | null } {
  const live = rows.filter(works).map(id);
  return { live, end: trunkEnd(key, rows.map(id), live) };
}

function lineClass(base: string, style: string, live: boolean, end = false): string {
  return [base, `flow-${style}`, live ? "live" : "", end ? "end" : ""].filter(Boolean).join(" ");
}

/**
 * The lit vertical line of a group, from below the header's badge to the elbow of its last
 * working session. A list item of its own, hidden from screen readers; first in the list, so the
 * rows keep their `:last-child`.
 */
export function TreeFlowTrunk({ live }: { live: readonly number[] }) {
  const flow = useTreeFlow();
  const level = useHighestLevel(live);
  if (flow === null) return null;
  return <TrunkItem kind="group" style={flow.style} live={live.length > 0} level={level} />;
}

/** The lit vertical line of a sub-agent branch, at the speed of its session. */
export function TreeFlowBranch({ sessionId, live }: { sessionId: number; live: boolean }) {
  const flow = useTreeFlow();
  const level = useFlowLevel(sessionId);
  if (flow === null) return null;
  return <TrunkItem kind="branch" style={flow.style} live={live} level={level} />;
}

function TrunkItem({
  kind,
  style,
  live,
  level,
}: {
  kind: "group" | "branch";
  style: string;
  live: boolean;
  level: FlowLevel;
}) {
  return (
    <li
      className={lineClass(`tree-flow-line ${kind}`, style, live)}
      data-flow-level={level}
      role="presentation"
      aria-hidden="true"
    />
  );
}

/**
 * The lit elbow of a session row (inside its tree guide) or of a sub-agent row. `end`: this
 * elbow ends the trunk (the trunk's anchor).
 */
export function TreeFlowElbow({
  sessionId,
  live,
  end,
}: {
  sessionId: number;
  live: boolean;
  end: boolean;
}) {
  const flow = useTreeFlow();
  const level = useFlowLevel(sessionId);
  if (flow === null) return null;
  return (
    <span
      className={lineClass("tree-flow-elbow", flow.style, live, end)}
      data-flow-level={level}
      aria-hidden="true"
    />
  );
}

/**
 * Output and input per second at the end of a row's line, in a cell of fixed width that is there
 * whenever the rate is turned on, so the line does not shift when a session starts or stops
 * working; the numbers show only while it works. Decorative: they change every second.
 */
export function TreeFlowRate({
  sessionId,
  live,
  language,
}: {
  sessionId: number;
  live: boolean;
  language: string;
}) {
  const flow = useTreeFlow();
  if (flow === null || !flow.showRate) return null;
  return <RateCell sessionId={sessionId} live={live} language={language} />;
}

function RateCell({
  sessionId,
  live,
  language,
}: {
  sessionId: number;
  live: boolean;
  language: string;
}) {
  const t = useT();
  const rate = useFlowRate(sessionId);
  const text = (bytes: number) => {
    const { unit, value } = rateParts(bytes, language);
    return t(unit === "mb" ? "treeFlow.rateMb" : "treeFlow.rateKb", { value });
  };
  return (
    <span className={live ? "tree-flow-rate live" : "tree-flow-rate"} aria-hidden="true">
      {rate !== undefined && (
        <>
          <span className="tree-flow-rate-down">{`↓ ${text(rate.down)}`}</span>
          <span className="tree-flow-rate-up">{`↑ ${text(rate.up)}`}</span>
        </>
      )}
    </span>
  );
}
