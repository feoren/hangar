// New versions of the installed programs (check_program_updates) and the outcome of an update
// session (event `program-update-result`). Both arrive as `unknown`; only complete, plausible
// entries get through. Contract: docs/wiki/pages/syntheses/program-updates.md.

import type { Translate } from "./i18n";

/** `run`: Hangar runs the update (one click). `copy`: the command is for copying only. */
export type UpdatePlanKind = "run" | "copy" | "none";

export const UPDATE_REASONS = [
  "invalid-hint",
  "not-installed",
  "foreign-launcher",
  "ambiguous",
  "unknown-installer",
  "no-package",
  "package-mismatch",
  "unsupported",
  "tool-missing",
] as const;
/** Why there is no `run` plan. */
export type UpdateReason = (typeof UPDATE_REASONS)[number];

export type UpdateWarning = "second-copy" | "running";

export interface ProgramUpdate {
  installed: string | null;
  latest: string | null;
  updateAvailable: boolean;
  plan: UpdatePlanKind;
  reason: UpdateReason | null;
  /** Machine-wide install: the update asks for administrator rights. */
  needsAdmin: boolean;
  /** The command that runs (`run`) or the text to copy (`copy`); display only. */
  command: string | null;
  warnings: UpdateWarning[];
  /** Every copy found, the started one first. */
  copies: readonly string[];
  /** Fingerprint of the plan the user sees; `create_update_session` refuses a changed plan. */
  planId: string | null;
  /** Unix time in ms of the query that `latest` comes from. */
  checkedAt: number;
}

export type ProgramUpdates = ReadonlyMap<string, ProgramUpdate>;

export const NO_UPDATES: ProgramUpdates = new Map();

/** Every 6 hours; the backend keeps the results in its disk cache for 24 h anyway. */
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/** Event after an update session ended and the version was read again. */
export const PROGRAM_UPDATE_RESULT = "program-update-result";

const MAX_VERSION_LENGTH = 40;
const MAX_HINT_LENGTH = 500;
const MAX_PATH_LENGTH = 4096;
const MAX_COPIES = 16;
const MAX_PLAN_ID_LENGTH = 200;

function shortText(value: unknown, max: number): string | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  return text.length > 0 && text.length <= max ? text : null;
}

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function fieldOf(value: object) {
  return (name: string): unknown => (name in value ? Reflect.get(value, name) : undefined);
}

function pathList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const result: string[] = [];
  for (const entry of value) {
    const path = shortText(entry, MAX_PATH_LENGTH);
    // eslint-disable-next-line no-control-regex
    if (path === null || /[\u0000-\u001f]/.test(path) || result.includes(path)) continue;
    result.push(path);
    if (result.length === MAX_COPIES) break;
  }
  return result;
}

function isReason(value: unknown): value is UpdateReason {
  return UPDATE_REASONS.some((reason) => reason === value);
}

function warningList(value: unknown): UpdateWarning[] {
  if (!Array.isArray(value)) return [];
  const entries: unknown[] = value;
  const result: UpdateWarning[] = [];
  for (const entry of entries) {
    if ((entry === "second-copy" || entry === "running") && !result.includes(entry)) {
      result.push(entry);
    }
  }
  return result;
}

function update(value: unknown): ProgramUpdate | null {
  const object = record(value);
  if (object === null) return null;
  const field = fieldOf(object);
  const available = field("updateAvailable");
  const checkedAt = field("checkedAt");
  if (typeof available !== "boolean") return null;
  if (typeof checkedAt !== "number" || !Number.isFinite(checkedAt)) return null;
  const latest = shortText(field("latest"), MAX_VERSION_LENGTH);
  const command =
    shortText(field("command"), MAX_HINT_LENGTH) ?? shortText(field("updateHint"), MAX_HINT_LENGTH);
  const rawPlan = field("plan");
  // Older backends know only `canRun` and `updateHint`; only an explicit `true` runs anything.
  let plan: UpdatePlanKind =
    rawPlan === "run" || rawPlan === "copy" || rawPlan === "none"
      ? rawPlan
      : field("canRun") === true
        ? "run"
        : command !== null
          ? "copy"
          : "none";
  // Nothing to run or copy without a command.
  if (plan !== "none" && command === null) plan = "none";
  const reason = field("reason");
  return {
    installed: shortText(field("installed"), MAX_VERSION_LENGTH),
    latest,
    // Without a known new version there is nothing to show, even if the field says otherwise.
    updateAvailable: available && latest !== null,
    plan,
    reason: plan === "run" || !isReason(reason) ? null : reason,
    needsAdmin: field("needsAdmin") === true,
    command,
    warnings: warningList(field("warnings")),
    copies: pathList(field("copies")),
    planId: shortText(field("planId"), MAX_PLAN_ID_LENGTH),
    checkedAt,
  };
}

export function parseProgramUpdates(value: unknown): ProgramUpdates {
  const result = new Map<string, ProgramUpdate>();
  const object = record(value);
  if (object === null) return result;
  for (const [id, entry] of Object.entries(object)) {
    const parsed = update(entry);
    if (parsed) result.set(id, parsed);
  }
  return result;
}

/** An update is available and Hangar can launch it itself. */
export function runnableUpdate(updates: ProgramUpdates, profileId: string): boolean {
  const entry = updates.get(profileId);
  return entry !== undefined && entry.updateAvailable && entry.plan === "run";
}

/**
 * The update offer below a program row already names the installed version ("1.2 → 1.3"), so
 * the row itself leaves it out then.
 */
export function offerShowsInstalled(update: ProgramUpdate | undefined): boolean {
  return (
    update !== undefined &&
    update.updateAvailable &&
    update.latest !== null &&
    update.plan === "run"
  );
}

/** New version if an update is available, otherwise `null`. */
export function availableVersion(updates: ProgramUpdates, profileId: string): string | null {
  const entry = updates.get(profileId);
  return entry?.updateAvailable ? entry.latest : null;
}

/** Translated sentence for a reason code. */
export function reasonText(reason: UpdateReason | null, t: Translate): string {
  switch (reason) {
    case "invalid-hint":
      return t("updateReason.invalidHint");
    case "not-installed":
      return t("updateReason.notInstalled");
    case "foreign-launcher":
      return t("updateReason.foreignLauncher");
    case "ambiguous":
      return t("updateReason.ambiguous");
    case "unknown-installer":
      return t("updateReason.unknownInstaller");
    case "no-package":
      return t("updateReason.noPackage");
    case "package-mismatch":
      return t("updateReason.packageMismatch");
    case "unsupported":
      return t("updateReason.unsupported");
    case "tool-missing":
      return t("updateReason.toolMissing");
    case null:
      return t("updateReason.unknown");
  }
}

/**
 * The short notes that go with an offer (why it cannot run, administrator rights, other copies),
 * in the order the full offer shows them. The compact sidebar offer puts them in its tooltip.
 */
export function updateNotes(update: ProgramUpdate, t: Translate): string[] {
  const notes: string[] = [];
  if (update.plan !== "run") notes.push(reasonText(update.reason, t));
  if (update.plan === "run" && update.needsAdmin) notes.push(t("update.needsAdmin"));
  if (update.warnings.includes("second-copy") && update.copies.length > 1) {
    notes.push(t("update.copiesFound", { count: update.copies.length }));
  }
  return notes;
}

// --- Outcome of an update session ---

export type OutcomeReason = "running" | "failed" | "shadowed" | "unchanged";

export interface UpdateOutcome {
  profileId: string;
  sessionId: number | null;
  code: number | null;
  before: string | null;
  after: string | null;
  changed: boolean | null;
  reason: OutcomeReason | null;
  copies: readonly string[];
}

function optionalInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) ? value : null;
}

/** Payload of `program-update-result`; `null` if malformed. */
export function parseUpdateOutcome(value: unknown): UpdateOutcome | null {
  const object = record(value);
  if (object === null) return null;
  const field = fieldOf(object);
  const profileId = shortText(field("profileId"), 200);
  if (profileId === null) return null;
  const changed = field("changed");
  const reason = field("reason");
  return {
    profileId,
    sessionId: optionalInteger(field("sessionId")),
    code: optionalInteger(field("code")),
    before: shortText(field("before"), MAX_VERSION_LENGTH),
    after: shortText(field("after"), MAX_VERSION_LENGTH),
    changed: typeof changed === "boolean" ? changed : null,
    reason:
      reason === "running" || reason === "failed" || reason === "shadowed" || reason === "unchanged"
        ? reason
        : null,
    copies: pathList(field("copies")),
  };
}

export interface OutcomeNotice {
  /** `ok`: updated or finished; `problem`: needs the user's attention. */
  tone: "ok" | "problem";
  message: string;
  /** Copies to show (another copy earlier on PATH). */
  paths: readonly string[];
}

/** Calm text for the notice after an update; `name` is the program's display name. */
export function outcomeNotice(outcome: UpdateOutcome, name: string, t: Translate): OutcomeNotice {
  const version = outcome.after ?? outcome.before ?? "?";
  switch (outcome.reason) {
    case "running":
      return { tone: "problem", message: t("updateOutcome.running", { name }), paths: [] };
    case "failed":
      return {
        tone: "problem",
        message:
          outcome.code === null
            ? t("updateOutcome.failedNoCode", { name })
            : t("updateOutcome.failed", { name, code: outcome.code }),
        paths: [],
      };
    case "shadowed":
      return {
        tone: "problem",
        message: t("updateOutcome.shadowed", { name, version }),
        paths: outcome.copies,
      };
    case "unchanged":
      return {
        tone: "problem",
        message: t("updateOutcome.unchanged", { name, version }),
        paths: [],
      };
    case null:
      if (outcome.changed === true && outcome.before !== null && outcome.after !== null) {
        return {
          tone: "ok",
          message: t("updateOutcome.updated", {
            name,
            before: outcome.before,
            after: outcome.after,
          }),
          paths: [],
        };
      }
      if (outcome.after !== null) {
        return {
          tone: "ok",
          message: t("updateOutcome.now", { name, version: outcome.after }),
          paths: [],
        };
      }
      return { tone: "ok", message: t("updateOutcome.finished", { name }), paths: [] };
  }
}

/** The backend refused because the plan changed since it was shown (ManagerError::PlanChanged). */
export function isPlanChanged(message: string): boolean {
  return message.includes("changed since it was shown");
}
