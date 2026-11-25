// Backend errors with a stable code: `[code] English text` (ManagerError in manager.rs,
// ArenaError in arena.rs, FileViewerError in file_viewer.rs), and for errors that name a path or a reason
// `[code] English text\n<detail>` (SnapshotError in git_snapshots.rs). Known codes are shown in
// the window's own words; the English text is the fallback for codes this window does not know
// yet. Arena codes are translated by `arenaErrorText` (arena.ts), snapshot codes by
// `timelineErrorText` (timeline.ts).

import { t, type MessageKey } from "./i18n";

const CODE = /^\[([a-z_]+)\] /;

const MESSAGES: ReadonlyMap<string, MessageKey> = new Map<string, MessageKey>([
  ["bad_cwd", "terminal.missingFolder"],
  ["remote_cwd", "terminal.remoteFolder"],
  ["viewer_limit", "files.tooManyViewers"],
  ["partner_name", "partnerError.name"],
  ["partner_folder", "partnerError.folder"],
  ["partner_missing_folder", "partnerError.missingFolder"],
  ["partner_self", "partnerError.self"],
  ["partner_nested", "partnerError.nested"],
  ["partner_name_taken", "partnerError.nameTaken"],
  ["partner_name_in_use", "partnerError.nameInUse"],
  ["partner_already", "partnerError.already"],
  ["partner_too_many", "partnerError.tooMany"],
  ["partner_not_partner", "partnerError.notPartner"],
  ["partner_no_project", "partnerError.noProject"],
  ["partner_wrong_target", "partnerError.wrongTarget"],
  ["partner_empty", "partnerError.empty"],
  ["partner_too_long", "partnerError.tooLong"],
  ["partner_forged_header", "partnerError.forgedHeader"],
  ["partner_invalid_list", "partnerError.invalidList"],
  ["partner_full", "partnerError.full"],
  ["partner_message_gone", "partnerError.messageGone"],
  ["partner_pair_full", "partnerError.pairFull"],
  ["partner_shell_sender", "partnerError.shellSender"],
  ["link_too_few", "linkError.tooFew"],
  ["link_too_many", "linkError.tooMany"],
  ["link_twice", "linkError.twice"],
  ["link_already", "linkError.already"],
  ["link_none_added", "linkError.noneAdded"],
  ["link_groups_full", "linkError.groupsFull"],
  ["link_not_linkable", "linkError.notLinkable"],
  ["link_unknown_group", "linkError.unknownGroup"],
  ["link_not_member", "linkError.notMember"],
  ["link_name", "links.nameInvalid"],
  ["link_name_reserved", "links.nameReserved"],
  ["link_name_taken", "links.nameTwice"],
  ["link_two_leads", "links.twoLeads"],
  ["link_message_gone", "partnerError.messageGone"],
  ["link_wrong_target", "linkError.wrongTarget"],
  ["link_claimed", "linkError.claimed"],
  ["notes_restore_old_key", "sharedNotes.restoreOldKey"],
]);

export interface BackendError {
  /** The stable code, `null` for an error without one. */
  code: string | null;
  /** The English text, without code and detail; the whole message when it has no code. */
  text: string;
  /** Everything after the first line break (a path, git's reason); `null` without one. */
  detail: string | null;
}

/** Takes a backend error apart; the English text of a coded error never holds a line break. */
export function splitBackendError(message: string): BackendError {
  const match = CODE.exec(message);
  if (match === null) return { code: null, text: message, detail: null };
  const rest = message.slice(match[0].length);
  const end = rest.indexOf("\n");
  return {
    code: match[1] ?? null,
    text: end < 0 ? rest : rest.slice(0, end),
    detail: end < 0 ? null : rest.slice(end + 1),
  };
}

/**
 * `message` as the user should read it: translated for a known code, else the English text
 * alone (a detail of an unknown code is left out).
 */
export function backendErrorText(message: string): string {
  const { code, text } = splitBackendError(message);
  if (code === null) return message;
  const key = MESSAGES.get(code);
  return key === undefined ? text : t(key);
}
