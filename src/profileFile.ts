// Make errors from reloading profiles.toml readable. The backend (`reload_profiles`, Rust:
// `ProfileProblem` in src-tauri/src/profile.rs) returns the message and, for syntax and field
// errors, line and column. Anything else (e.g. a plain string error) is shown as usual.

import { errorText } from "./api";
import type { Translate } from "./i18n";

export interface ProfileProblem {
  message: string;
  /** From 1; `null` if the error has no position in the file (e.g. a duplicate ID). */
  line: number | null;
  column: number | null;
}

function position(value: unknown): number | null | undefined {
  if (value === null) return null;
  return typeof value === "number" && Number.isInteger(value) && value > 0 ? value : undefined;
}

/**
 * Checks the rejection of `reloadProfiles`; anything that does not fit yields `null`. The backend
 * always sends `line` and `column` (`null` if need be); a JavaScript `Error` does have
 * `message`, but it is not a profile error.
 */
export function parseProfileProblem(error: unknown): ProfileProblem | null {
  if (typeof error !== "object" || error === null || Array.isArray(error)) return null;
  if (error instanceof Error || !("line" in error) || !("column" in error)) return null;
  const message: unknown = Reflect.get(error, "message");
  const line = position(Reflect.get(error, "line"));
  const column = position(Reflect.get(error, "column"));
  if (typeof message !== "string" || line === undefined || column === undefined) return null;
  return { message, line, column };
}

export function describeProfileProblem(problem: ProfileProblem, t: Translate): string {
  if (problem.line === null) return t("profiles.problem", { message: problem.message });
  return t("profiles.problemAt", {
    line: problem.line,
    column: problem.column ?? 1,
    message: problem.message,
  });
}

/** Text for any error from `reloadProfiles` (or `openProfileFile`). */
export function profileErrorText(error: unknown, t: Translate): string {
  const problem = parseProfileProblem(error);
  return problem ? describeProfileProblem(problem, t) : errorText(error);
}

/**
 * Reloading on returning to the window reports an error only once: anyone who has not fixed the
 * file yet does not get the same message on every window switch. `lastReported` is the text
 * reported last, `null` after a successful load.
 */
export function shouldReportReloadError(lastReported: string | null, text: string): boolean {
  return lastReported !== text;
}
