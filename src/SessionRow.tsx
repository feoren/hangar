// The inside of a session row in the sidebar: title line, folder line, load and context, laid out
// as the chosen row style asks (rowStyle.ts). Pure: the row (Sidebar.tsx SessionItem) reads the
// stores and passes the numbers, so the settings' preview draws the very same markup. The standard
// style's markup is the row's long-standing one; the other styles add only the parts they need
// and leave the rest to styles.css ("Row styles").

import type { ReactNode } from "react";

import { Badge } from "./Badge";
import { ContextLineCells, ContextMini, ContextRing, RuntimeCell } from "./ContextLine";
import { contextShare, formatShare, type SessionContext } from "./contextUsage";
import { useT, type MessageKey } from "./i18n";
import { BranchIcon, ChevronIcon, FolderIcon, TeamIcon } from "./icons";
import { revealInvisible } from "./invisible";
import type { Marker } from "./marker";
import type { BadgeLook } from "./programPrefs";
import type { ResourceParts } from "./resources";
import { Revealed } from "./Revealed";
import type { RowParts, StateCount } from "./rowStyle";
import type { ChipKind } from "./sessionChip";
import { SessionLoad } from "./SessionLoad";
import { SessionMarker } from "./SessionMarker";
import type { StatusLine } from "./statusLine";
import { StatusChip } from "./StatusChip";
import { shortBranch } from "./worktree";

export interface RowBodyProps {
  parts: RowParts;
  name: string;
  /** The program's badge before the title in a project group; `null` in a program group. */
  badge: BadgeLook | null;
  /** The program's badge for the avatar styles. */
  program: BadgeLook;
  marker: Marker | undefined;
  line: StatusLine;
  waiting: boolean;
  chip: ChipKind | null;
  /** The small signs before the title (problems, layout glyph, dev server, teammates). */
  signs: ReactNode;
  /** The folder on the second line; `null`: none. */
  place: string | null;
  /** The git branch, the whole name; `null`: none. */
  branch: string | null;
  /** The branch is a team session's worktree branch. */
  team: boolean;
  /** CPU, RAM and GPU; `null` while the setting is off or the session has ended. */
  usage: ResourceParts | null;
  gpuColumn: boolean;
  runtime: string | null;
  context: SessionContext | undefined;
  language: string;
  /** Id of the folding details, for the chevron's aria-controls (accordion style). */
  moreId?: string | undefined;
}

export function SessionRowBody(props: RowBodyProps) {
  const { parts, usage, runtime, context, language } = props;
  const loadLine =
    parts.load !== "none" && (usage !== null || (parts.runtime === "load" && runtime !== null));
  const lines = (
    <>
      {/* Second line: where it runs, the folder at the start and the branch at the right
          edge. The folder gives way first, then the branch, both with an ellipsis (styles.css,
          "Second line"). The compact style adds its small context bar. */}
      <span className="session-meta">
        <RowWhere place={props.place} branch={props.branch} team={props.team} />
        {parts.mini === "meta" && <ContextMini context={context} language={language} withShare />}
      </span>
      {/* Third line: the tree's load (while the setting is on), each value in a column of fixed
          width; fourth line: run time and context window. Each line exists only with its
          numbers, so a row never keeps an empty line (styles.css, "Third line"). */}
      {loadLine && (
        <span className="session-detail">
          {usage !== null && (
            <SessionLoad usage={usage} gpuColumn={props.gpuColumn} tiles={parts.load === "tiles"} />
          )}
          {parts.runtime === "load" && runtime !== null && <RuntimeCell runtime={runtime} />}
        </span>
      )}
      <ContextLineCells
        runtime={runtime}
        context={context}
        language={language}
        meter={parts.meter}
        runtimeCell={parts.runtime === "context"}
      />
    </>
  );
  return (
    <span className="session-text">
      {parts.avatar !== null && (
        <span className="session-avatar" aria-hidden="true">
          <Badge look={props.program} />
          {parts.avatar === "dot" && <span className="state-dot" />}
        </span>
      )}
      {parts.ring === "start" && <ContextRing context={context} />}
      <TitleLine {...props} />
      {parts.expander ? (
        <span className="session-more" id={props.moreId}>
          <span className="session-more-inner">{lines}</span>
        </span>
      ) : (
        lines
      )}
      {parts.ring === "end" && <ContextRing context={context} />}
    </span>
  );
}

/**
 * First line: the title in the middle, its state as one word at the right edge (in line with the
 * branch below and the last value of the lines below), the small signs in front of the title. The
 * DOM keeps title, then state, then the signs, so screen readers hear them in that order; CSS
 * `order` puts the signs before the title (styles.css, "First line"). Only the title gives way.
 */
function TitleLine({
  parts,
  name,
  badge,
  marker,
  line,
  waiting,
  chip,
  signs,
  usage,
  runtime,
  context,
  language,
}: RowBodyProps) {
  const t = useT();
  const statusClass = waiting
    ? "session-status attention-text"
    : line.stalled === true
      ? "session-status stalled"
      : "session-status";
  return (
    <span className="session-title-row">
      {badge !== null && <Badge look={badge} />}
      <SessionMarker marker={marker} tooltip={false} />
      <span className="session-title">{name}</span>
      {parts.mini === "title" && (
        <ContextMini context={context} language={language} withShare={false} />
      )}
      {/* One word in a cell of fixed width; the whole sentence with the program and how long
          ("Claude Code has been working for 3 min") is for screen readers here and for everyone
          in the peek. Styles without the word say the state by shape before the title. */}
      <span className="session-state">
        {parts.end === "word" && (
          <>
            <span className="state-dot" aria-hidden="true" />
            <span className={statusClass} aria-hidden="true">
              {line.word}
            </span>
          </>
        )}
        {parts.end === "runtime" && runtime !== null && (
          <>
            <span className="session-state-runtime" aria-hidden="true">
              {runtime}
            </span>
            <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
          </>
        )}
        {parts.end === "columns" && (
          <RowColumns
            cpu={usage?.cpu ?? null}
            context={context}
            runtime={runtime}
            language={language}
          />
        )}
        <span className="sr-only">{line.full}</span>
      </span>
      {signs}
      {parts.end !== "word" && (
        <span className="session-shape" aria-hidden="true">
          {chip === null ? <span className="state-dot" /> : <StatusChip kind={chip} />}
        </span>
      )}
    </span>
  );
}

/** Folder and branch of the second line; nothing without either. */
export function RowWhere({
  place,
  branch,
  team,
}: {
  place: string | null;
  branch: string | null;
  team: boolean;
}) {
  const t = useT();
  if (place === null && branch === null) return null;
  return (
    <span className="session-where">
      {place !== null && (
        <span className="session-path">
          <span className="session-path-icon" aria-hidden="true">
            <FolderIcon size={11} />
          </span>
          <span className="session-path-name">{place}</span>
        </span>
      )}
      {branch !== null &&
        (team ? (
          <span className="session-branch worktree">
            <TeamIcon size={11} />
            <span className="sr-only">
              {t("worktree.sessionHint", { branch: revealInvisible(branch) })}
            </span>
            {/* The icon already says "team session"; the "hangar/" prefix would only repeat it. */}
            <span className="session-branch-name" aria-hidden="true">
              <Revealed text={shortBranch(branch)} />
            </span>
          </span>
        ) : (
          <span className="session-branch">
            <BranchIcon size={11} />
            <span className="sr-only">
              {t("session.branch", { branch: revealInvisible(branch) })}
            </span>
            <span className="session-branch-name" aria-hidden="true">
              <Revealed text={branch} />
            </span>
          </span>
        ))}
    </span>
  );
}

/**
 * The columns of the "columns" style at the title line's end: CPU, context share and run time,
 * each in a cell of fixed width, named once above the list (RowColumnsHead).
 */
function RowColumns({
  cpu,
  context,
  runtime,
  language,
}: {
  cpu: string | null;
  context: SessionContext | undefined;
  runtime: string | null;
  language: string;
}) {
  const t = useT();
  return (
    <span className="session-columns">
      <span className="session-column session-column-cpu" aria-hidden="true">
        {cpu}
      </span>
      <span className="session-column session-column-context" aria-hidden="true">
        {context === undefined ? null : formatShare(contextShare(context), language)}
      </span>
      <span className="session-column session-column-runtime" aria-hidden="true">
        {runtime}
      </span>
      {cpu !== null && <span className="sr-only">{`${t("system.cpu")} ${cpu}`}</span>}
      {runtime !== null && (
        <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
      )}
    </span>
  );
}

/**
 * The column names of the "columns" style, once above the session list. Decorative: every row
 * says its values to screen readers with their names.
 */
export function RowColumnsHead() {
  const t = useT();
  return (
    <div className="session-columns-head" aria-hidden="true">
      <span className="session-columns-head-name">{t("sidebar.sessions")}</span>
      <span className="session-column session-column-cpu">{t("system.cpu")}</span>
      <span className="session-column session-column-context">{t("rowStyle.columnContext")}</span>
      <span className="session-column session-column-runtime">{t("rowStyle.columnTime")}</span>
    </div>
  );
}

/**
 * The chevron of a folding row (accordion style): shows or hides its other lines. A button of its
 * own beside the row's main button, as buttons do not nest.
 */
export function RowExpander({
  expanded,
  controls,
  onToggle,
}: {
  expanded: boolean;
  controls: string;
  onToggle: () => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      className="session-expander"
      aria-expanded={expanded}
      aria-controls={controls}
      aria-label={t("rowStyle.details")}
      title={t("rowStyle.details")}
      onClick={onToggle}
    >
      <ChevronIcon size={10} />
    </button>
  );
}

/** One word per state, for the counts' screen reader text and tooltip. */
const CHIP_WORDS: Record<ChipKind, MessageKey> = {
  working: "status.short.working",
  asks: "status.short.asks",
  done: "status.short.attention",
  failed: "status.short.failed",
  paused: "status.short.idle",
  ended: "status.short.exited",
};

/** "working: 2, question: 1" for the group header's tooltip and screen readers. */
export function stateCountsText(counts: readonly StateCount[], t: ReturnType<typeof useT>): string {
  return counts
    .map(({ kind, count }) => t("rowStyle.stateCount", { state: t(CHIP_WORDS[kind]), count }))
    .join(", ");
}

/**
 * The counts per state beside a group header's session count ("counters" style): each state's
 * shape with its number, so the shape and not the color alone says which state it counts.
 */
export function GroupStateCounts({ counts }: { counts: readonly StateCount[] }) {
  const t = useT();
  if (counts.length === 0) return null;
  return (
    <span className="agent-group-states">
      {counts.map(({ kind, count }) => (
        <span key={kind} className={`agent-group-state ${kind}`} aria-hidden="true">
          <StatusChip kind={kind} />
          <span className="agent-group-state-count">{count}</span>
        </span>
      ))}
      <span className="sr-only">{stateCountsText(counts, t)}</span>
    </span>
  );
}
