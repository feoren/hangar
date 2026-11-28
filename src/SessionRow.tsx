// The inside of a session row in the sidebar, drawn from the row layout the user built
// (rowLayout.ts): up to four lines, each with its elements from the left and those at its right
// edge, every element in its chosen form. Pure: the row (Sidebar.tsx SessionItem) reads the stores
// and passes the numbers, so the settings' preview draws the very same markup (styles.css,
// "Row layout").

import { Fragment, type ReactNode } from "react";

import { Badge } from "./Badge";
import { ContextFact, ContextRing, ContextTokens, RuntimeCell } from "./ContextLine";
import {
  contextHint,
  formatOutputRate,
  outputRateHint,
  type OutputRate,
  type SessionContext,
} from "./contextUsage";
import { useT } from "./i18n";
import { BranchIcon, FolderIcon, TeamIcon } from "./icons";
import { revealInvisible } from "./invisible";
import type { Marker } from "./marker";
import type { BadgeLook } from "./programPrefs";
import type { ResourceParts } from "./resources";
import { Revealed } from "./Revealed";
import type { PlannedLine, RowElement, RowPlan } from "./rowLayout";
import type { ChipKind } from "./sessionChip";
import { SessionLoad } from "./SessionLoad";
import { SessionLook } from "./SessionLook";
import { SessionMarker } from "./SessionMarker";
import type { StatusLine } from "./statusLine";
import { StatusChip } from "./StatusChip";
import { shortBranch } from "./worktree";

export interface RowBodyProps {
  plan: RowPlan;
  name: string;
  /** The program's badge before the title in a project group; `null` in a program group. */
  badge: BadgeLook | null;
  /** The program's badge for the avatar form, in every group. */
  program: BadgeLook;
  marker: Marker | undefined;
  line: StatusLine;
  waiting: boolean;
  chip: ChipKind | null;
  /** The small signs (problems, layout glyph, dev server, teammates); `null` or `false`: none. */
  signs: ReactNode;
  /** The folder; `null`: none. */
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
  /** The estimated cost so far ("~$0.42", costs.ts `costText`); missing or `null`: none. */
  cost?: string | null;
  /**
   * Output tokens per second of Claude Code (useContextUsage.ts), live or the last one kept;
   * missing or `null`: none measured yet.
   */
  speed?: OutputRate | null;
  /**
   * The model the session started with: the badge's short text and the full name for its
   * tooltip (localLlm.ts `modelBadge`, `modelNote`); missing or `null`: the program's default.
   */
  model?: { text: string; title: string } | null;
  language: string;
  /**
   * What decides a sub-agent session's figure (SessionLook.tsx): with `subAgent` the figure
   * takes the badge's place in every form, also in a program group. Missing: the badge.
   */
  session?: { subAgent?: string | null; cwd: string };
  /** The program's name for the figure's tooltip. */
  programName?: string;
  /** Before the title, after the marker: the lead's crown of a link group (LinkMark.tsx). */
  beforeTitle?: ReactNode;
  /** Read after the title by screen readers only: how the row is linked (LinkMark.tsx). */
  titleNote?: string | null;
  /**
   * The tree flow's rate cell (TreeFlowView.tsx), at the right end of one line: the load's line, else
   * the last line below the title. Missing or `null`: none. It never makes a line show.
   */
  rate?: ReactNode;
  /**
   * The running sub-agents' dot (SubAgentCount.tsx), decorative, on the corner of the program's
   * badge wherever the row shows it; outside the flow, so it takes no width. Missing or `null`:
   * none.
   */
  badgeCount?: ReactNode;
  /**
   * Hangar Drive's logo (DriveMark.tsx) right after the folder's name, on the folder's line; it
   * stays whole while the name gives way. Missing or `null`: none, and none without a folder.
   */
  drive?: ReactNode;
}

function hasFigure(props: RowBodyProps): boolean {
  return (props.session?.subAgent ?? null) !== null;
}

/** The badge, or a sub-agent session's figure with the badge on its corner. */
function Look({
  look,
  crop,
  ...props
}: { look: BadgeLook; crop?: "full" | "head" } & RowBodyProps) {
  const shown =
    props.session === undefined ? (
      <Badge look={look} />
    ) : (
      <SessionLook
        look={look}
        session={props.session}
        {...(crop === undefined ? {} : { crop })}
        {...(props.programName === undefined ? {} : { programName: props.programName })}
      />
    );
  if (props.badgeCount === undefined || props.badgeCount === null) return shown;
  return (
    <span className="badge-anchor">
      {shown}
      {props.badgeCount}
    </span>
  );
}

/** Whether an element has anything to show in this row; a line with none of them is left out. */
function present(element: RowElement, props: RowBodyProps): boolean {
  const { plan } = props;
  switch (element) {
    case "title":
    case "state":
      return true;
    case "program":
      // The small icon only where the group header does not name the program; a sub-agent's
      // figure always, since the header names the program, not the agent.
      return plan.layout.forms.program === "avatar" || props.badge !== null || hasFigure(props);
    case "signs":
      return props.signs !== null && props.signs !== false && props.signs !== undefined;
    case "folder":
      return props.place !== null;
    case "branch":
      return props.branch !== null;
    case "load":
      return props.usage !== null;
    case "runtime":
      return props.runtime !== null;
    case "context":
      return plan.layout.forms.context === "ring" || props.context !== undefined;
    case "tokens":
      return props.context !== undefined;
    case "cost":
      return props.cost !== undefined && props.cost !== null;
    case "speed":
      return props.speed !== undefined && props.speed !== null;
    case "model":
      return props.model !== undefined && props.model !== null;
  }
}

/** Which planned line takes the rate cell; -1: none shows. */
export function rateLine(
  lines: readonly PlannedLine[],
  present: (e: RowElement) => boolean,
): number {
  const shown = lines
    .map((line, index) => ({ index, title: line.title, all: [...line.left, ...line.right] }))
    .filter((line) => line.all.some(present));
  const load = present("load") ? shown.find((line) => line.all.includes("load")) : undefined;
  const below = shown.filter((line) => !line.title);
  return (load ?? below[below.length - 1] ?? shown[shown.length - 1])?.index ?? -1;
}

export function SessionRowBody(props: RowBodyProps) {
  const { plan, context, language } = props;
  const t = useT();
  const hint = context === undefined ? null : contextHint(context, language, t);
  const rateAt =
    props.rate === undefined || props.rate === null
      ? -1
      : rateLine(plan.lines, (element) => present(element, props));
  const className = [
    "session-text",
    plan.lead === null ? "" : "has-lead",
    plan.trail === null ? "" : "has-trail",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={className}>
      {plan.lead !== null && <Spanning kind={plan.lead} end={false} {...props} />}
      {plan.lines.map((line, index) => (
        <RowLineView key={index} planned={line} withRate={index === rateAt} {...props} />
      ))}
      {plan.trail !== null && <Spanning kind={plan.trail} end {...props} />}
      {/* The context's exact numbers, once, wherever and however the row shows it. */}
      {hint !== null && <span className="sr-only">{hint}</span>}
    </span>
  );
}

/** The avatar or ring beside the first two lines, at the row's start or its end. */
function Spanning({
  kind,
  end,
  ...props
}: { kind: "avatar" | "ring"; end: boolean } & RowBodyProps) {
  return (
    <span className={end ? "row-span row-trail" : "row-span row-lead"}>
      {kind === "ring" ? (
        <ContextRing context={props.context} />
      ) : (
        <Avatar {...props} look={props.program} />
      )}
    </span>
  );
}

function Avatar({ look, ...props }: { look: BadgeLook } & RowBodyProps) {
  return (
    <span className="session-avatar" aria-hidden="true">
      <Look look={look} crop="full" {...props} />
      <span className="state-dot" />
    </span>
  );
}

/**
 * One line: its left elements, then a spacer that pushes the right ones to the edge. A line
 * without anything to show is gone. On the title's line the DOM keeps the title, then what follows
 * it, then what stands before it, so screen readers hear the title first; CSS `order` puts those
 * elements back in front (styles.css, "First line"). A run time keeps its empty cell on a line
 * that shows, so what follows it starts at the same place in every row.
 */
function RowLineView({
  planned: line,
  withRate,
  ...props
}: { planned: PlannedLine; withRate: boolean } & RowBodyProps) {
  const shows = (element: RowElement) => element === "runtime" || present(element, props);
  if (![...line.left, ...line.right].some((element) => present(element, props))) return null;
  const left = line.left.filter(shows);
  const right = line.right.filter(shows);
  const titleAt = line.titleLeft ? left.indexOf("title") : -1;
  const before = titleAt > 0 ? left.slice(0, titleAt) : [];
  const after = titleAt >= 0 ? left.slice(titleAt) : left;
  const className = [
    "row-line",
    line.title ? "session-title-row" : "",
    line.beside === null ? "" : `beside beside-${String(line.beside)}`,
  ]
    .filter(Boolean)
    .join(" ");
  const element = (id: RowElement) => (
    <Fragment key={id}>
      <RowElementView element={id} {...props} />
    </Fragment>
  );
  return (
    <span className={className}>
      {after.map(element)}
      {(right.length > 0 || withRate) && <span className="row-push" aria-hidden="true" />}
      {right.map(element)}
      {withRate && props.rate}
      {before.length > 0 && <span className="row-before-title">{before.map(element)}</span>}
    </span>
  );
}

/** One element in its form. */
function RowElementView({ element, ...props }: { element: RowElement } & RowBodyProps) {
  const forms = props.plan.layout.forms;
  switch (element) {
    case "title":
      return (
        <>
          <SessionMarker marker={props.marker} tooltip={false} />
          {props.beforeTitle}
          <span className="session-title">{props.name}</span>
          {/* The whole sentence with the program and how long ("Claude Code has been working
              for 3 min"), whatever the state's form, also while it is hidden. */}
          <span className="sr-only">{props.line.full}</span>
          {props.titleNote != null && <span className="sr-only">{props.titleNote}</span>}
        </>
      );
    case "state":
      return <StateView {...props} />;
    case "program":
      if (forms.program === "avatar") return <Avatar {...props} look={props.program} />;
      if (props.badge === null && hasFigure(props)) return <Look {...props} look={props.program} />;
      return props.badge === null ? null : <Look {...props} look={props.badge} />;
    case "signs":
      return props.signs;
    case "folder":
      return props.place === null ? null : (
        <RowFolder place={props.place} tag={forms.folder === "tag"} drive={props.drive ?? null} />
      );
    case "branch":
      return props.branch === null ? null : (
        <RowBranch branch={props.branch} team={props.team} tag={forms.branch === "tag"} />
      );
    case "load":
      return props.usage === null ? null : (
        <SessionLoad usage={props.usage} gpuColumn={props.gpuColumn} form={forms.load} />
      );
    case "runtime":
      return <RuntimeCell runtime={props.runtime} />;
    case "context":
      return <ContextFact context={props.context} language={props.language} form={forms.context} />;
    case "tokens":
      return <ContextTokens context={props.context} />;
    case "cost":
      return props.cost === undefined || props.cost === null ? null : <RowCost cost={props.cost} />;
    case "speed":
      return props.speed === undefined || props.speed === null ? null : (
        <RowSpeed rate={props.speed} language={props.language} />
      );
    case "model":
      return props.model === undefined || props.model === null ? null : (
        <span className="session-model row-fact" title={props.model.title}>
          <span className="sr-only">{props.model.title}</span>
          <span aria-hidden="true">{props.model.text}</span>
        </span>
      );
  }
}

/**
 * The output rate on screen ("84 tok/s"), quieter once it is the last response's and no longer
 * live; the sentence for screen readers.
 */
function RowSpeed({ rate, language }: { rate: OutputRate; language: string }) {
  const t = useT();
  const value = formatOutputRate(rate.value, language);
  return (
    <span className={rate.live ? "session-speed row-fact" : "session-speed row-fact settled"}>
      <span className="sr-only">{outputRateHint(rate, language, t)}</span>
      <span aria-hidden="true">{t("speed.value", { value })}</span>
    </span>
  );
}

/** The estimated cost, with its name for screen readers ("Cost ~$0.42"). */
function RowCost({ cost }: { cost: string }) {
  const t = useT();
  return (
    <span className="session-cost row-fact">
      <span className="sr-only">{t("rowElement.cost")} </span>
      {cost}
    </span>
  );
}

/**
 * The state in its form. Decorative: the title's sentence says it in words. The word keeps a cell
 * of fixed width, so what stands before it ends at the same place in every row.
 */
function StateView({ plan, line, waiting, chip }: RowBodyProps) {
  const form = plan.state ?? "word";
  if (form === "symbol") {
    return (
      <span className="session-shape" aria-hidden="true">
        {chip === null ? <span className="state-dot" /> : <StatusChip kind={chip} />}
      </span>
    );
  }
  const statusClass = waiting
    ? "session-status attention-text"
    : line.stalled === true
      ? "session-status stalled"
      : "session-status";
  // The tint, the band and the avatar's corner dot already show the state's color.
  const dot = (form === "word" || form === "pill") && !plan.avatar;
  return (
    <span className={`session-state state-${form}`} aria-hidden="true">
      {dot && <span className="state-dot" />}
      <span className={statusClass}>{line.word}</span>
    </span>
  );
}

function RowFolder({ place, tag, drive }: { place: string; tag: boolean; drive: ReactNode }) {
  const className = [
    "session-path",
    "row-where",
    tag ? "tag" : "",
    drive === null ? "" : "has-drive",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={className}>
      <span className="session-path-icon" aria-hidden="true">
        <FolderIcon size={11} />
      </span>
      <span className="session-path-name">{place}</span>
      {drive}
    </span>
  );
}

function RowBranch({ branch, team, tag }: { branch: string; team: boolean; tag: boolean }) {
  const t = useT();
  const className = ["session-branch", "row-where", team ? "worktree" : "", tag ? "tag" : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={className}>
      {team ? <TeamIcon size={11} /> : <BranchIcon size={11} />}
      <span className="sr-only">
        {team
          ? t("worktree.sessionHint", { branch: revealInvisible(branch) })
          : t("session.branch", { branch: revealInvisible(branch) })}
      </span>
      {/* A team session's icon already says "team session"; the "hangar/" prefix would only
          repeat it. */}
      <span className="session-branch-name" aria-hidden="true">
        <Revealed text={team ? shortBranch(branch) : branch} />
      </span>
    </span>
  );
}
