// The notice after restoring the sessions of a run that did not quit cleanly (crash, kill, power
// loss; Rust: exit_marker.rs). Pure: what the restore brought back, how that stands now and the
// text of the notice. Contract: docs/wiki/pages/syntheses/session-restore.md, "After a crash".

import type { Translate } from "./i18n";

/** How one saved session came back at the start. */
export type RestoreStart = "resumed" | "started" | "placeholder";

export interface RestoredSession {
  /** Session id after the restore; a placeholder has a negative one. */
  id: number;
  start: RestoreStart;
}

/** Enough of a session in the list to judge it now. */
export interface SessionNow {
  id: number;
  title: string;
  /** `false` after a `resumeFallback`, which may come seconds after the start. */
  resumed?: boolean | undefined;
}

export interface CrashSummary {
  /** Sessions running again, with or without their conversation. */
  restored: number;
  /** Of those, the ones whose agent resumed its conversation. */
  resumed: number;
  /** Titles of placeholders: the program is missing or did not start. */
  notStarted: string[];
  /** Titles of sessions whose resume failed and that started fresh. */
  fellBack: string[];
}

/**
 * The restore as it stands in `sessions`: a session closed since does not count any more, nor
 * does a placeholder that was restarted (it has a new id). `null` once nothing is left to tell.
 */
export function crashSummary(
  starts: readonly RestoredSession[],
  sessions: readonly SessionNow[],
): CrashSummary | null {
  const byId = new Map(sessions.map((session) => [session.id, session]));
  const summary: CrashSummary = { restored: 0, resumed: 0, notStarted: [], fellBack: [] };
  let counted = 0;
  for (const start of starts) {
    const session = byId.get(start.id);
    if (session === undefined) continue;
    counted += 1;
    if (start.start === "placeholder") {
      summary.notStarted.push(session.title);
      continue;
    }
    summary.restored += 1;
    if (start.start !== "resumed") continue;
    if (session.resumed === true) summary.resumed += 1;
    else summary.fellBack.push(session.title);
  }
  return counted === 0 ? null : summary;
}

/** Whether the notice names sessions that did not come back whole; it then stays until closed. */
export function hasProblems(summary: CrashSummary): boolean {
  return summary.notStarted.length > 0 || summary.fellBack.length > 0;
}

/** The notice's lines: the headline, then one line per kind of problem. */
export function crashNoticeLines(summary: CrashSummary, t: Translate, language: string): string[] {
  const { restored, resumed } = summary;
  const headline =
    restored === 0
      ? t("crash.noneRestored")
      : resumed > 0
        ? restored === 1
          ? t("crash.resumedOne")
          : t("crash.resumedMany", { count: restored })
        : restored === 1
          ? t("crash.restoredOne")
          : t("crash.restoredMany", { count: restored });
  const list = new Intl.ListFormat(language, { type: "conjunction" });
  const lines = [headline];
  if (summary.notStarted.length > 0) {
    lines.push(t("crash.notStarted", { names: list.format(summary.notStarted) }));
  }
  if (summary.fellBack.length > 0) {
    lines.push(t("crash.fellBack", { names: list.format(summary.fellBack) }));
  }
  return lines;
}
