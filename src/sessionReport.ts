// The session report: the agent's own transcript as Markdown (`export_report`, report/ in the
// backend). Here: which sessions offer it, the headings in the window's language and the UTC
// offset for the times shown.

import type { ReportLabels } from "./api";
import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";

/**
 * The backend can read this session's transcript: the same condition as the sub-agent view
 * (the program keeps transcripts Hangar finds, Claude Code so far, and the session has a
 * conversation id). Codex and every other program are left out, not shown disabled.
 */
export function canExportReport(session: Pick<SessionView, "subAgents">): boolean {
  return session.subAgents === true;
}

export function reportLabels(t: Translate): ReportLabels {
  return {
    program: t("report.program"),
    folder: t("report.folder"),
    branch: t("report.branch"),
    started: t("report.started"),
    lastActivity: t("report.lastActivity"),
    duration: t("report.duration"),
    conversation: t("report.conversation"),
    subAgent: t("report.subAgent"),
    tokens: t("report.tokens"),
    compacted: t("report.compacted"),
    earlierLeftOut: t("report.earlierLeftOut"),
    changedFiles: t("report.changedFiles"),
    commits: t("report.commits"),
    none: t("report.none"),
  };
}

/** Minutes east of UTC (`getTimezoneOffset` counts west), e.g. 60 for UTC+01:00. */
export function utcOffsetMinutes(date: Date): number {
  return -date.getTimezoneOffset() || 0;
}
