// Agent commands (Rust: agent_commands.rs): short texts such as `/compact` that the session menu,
// quick access and the context meter's compact button type into a running session of a program.
// The definitions come from the backend with each profile (built-in catalog plus the user's own
// in profiles.toml); this module only decides where they show and how they are sent. Pure, so
// the rules are tested without React. Contract: docs/wiki/pages/syntheses/agent-commands.md.

import type { AgentCommand, AgentCommandLabel, Profile } from "./api";
import type { AttentionReason, AttentionState } from "./attention";
import { HIGH_SHARE } from "./contextUsage";
import { blockedFor, sendToTargets, type BroadcastTarget } from "./broadcast";
import type { MessageKey, Translate } from "./i18n";
import type { LauncherItem } from "./LauncherDialog";
import { looksLikeQuestion } from "./promptGuard";
import type { SessionView } from "./sessions";

const LABELS: Record<AgentCommandLabel, MessageKey> = {
  compact: "agentCommand.compact",
  clear: "agentCommand.clear",
  new: "agentCommand.new",
  model: "agentCommand.model",
  usage: "agentCommand.usage",
  context: "agentCommand.context",
  status: "agentCommand.status",
  stats: "agentCommand.stats",
};

/** What a command needs from its session. */
type CommandSession = Pick<SessionView, "profileId" | "status" | "kind">;

/** The menu name of a command: Hangar's translation of its key, or the user's own label. */
export function commandLabel(command: AgentCommand, t: Translate): string {
  if (command.labelKey !== undefined) return t(LABELS[command.labelKey]);
  return command.label ?? command.text;
}

/**
 * The commands a session offers right now: those of its program while its process runs. Ended,
 * starting and update sessions offer none, so the menu entry is left out rather than greyed.
 */
export function sessionCommands(
  session: CommandSession,
  profiles: readonly Pick<Profile, "id" | "agentCommands">[],
): AgentCommand[] {
  if (session.status !== "running" || session.kind === "update") return [];
  return profiles.find((profile) => profile.id === session.profileId)?.agentCommands ?? [];
}

/** The program's compact command (label key `compact`), if it has one. */
export function compactCommand(commands: readonly AgentCommand[]): AgentCommand | null {
  return commands.find((command) => command.labelKey === "compact") ?? null;
}

/**
 * The agent is not ready for a typed command: it works on a turn, or it rang the bell or waits
 * for a gate decision, which usually means a question on screen. Enter would answer that question
 * (for example approve a tool) instead of sending the command.
 */
export function commandBusy(state: AttentionState, reason: AttentionReason | null): boolean {
  return state === "working" || awaitsAnswer(reason);
}

/**
 * The session rang the bell, waits for a gate decision or shows a dialog its hooks reported
 * ("permission", ADR 0016): there may be a question on screen, and the Enter after a command
 * would answer it. No command is offered until the person has looked (the reason clears then)
 * or the dialog is gone. Menu and quick access still send while the agent merely works.
 */
export function awaitsAnswer(reason: AttentionReason | null): boolean {
  return reason === "bell" || reason === "gate" || reason === "permission";
}

/**
 * `awaitsAnswer`, or the newest screen lines show a question or a choice list (promptGuard.ts, the
 * queued message's guard). Permission dialogs of Claude Code and Codex ring no bell in Hangar's
 * terminal, so neither the bell nor the gate reveals them, and the Enter after a command would
 * confirm the preselected answer. `screen` is read only when the reason does not decide already,
 * and only where a command is offered or sent, never on every render.
 */
export function asksQuestion(
  reason: AttentionReason | null,
  screen: () => readonly string[],
): boolean {
  return awaitsAnswer(reason) || looksLikeQuestion(screen());
}

/**
 * The compact button shows from the meter's warning share on, for programs with a compact
 * command, and not while the agent is busy (`commandBusy`): Codex refuses `/compact` during a
 * turn, and an Enter into a question would answer it.
 */
export function showCompact(
  share: number | null,
  commands: readonly AgentCommand[],
  busy: boolean,
): boolean {
  return share !== null && share >= HIGH_SHARE && !busy && compactCommand(commands) !== null;
}

/**
 * A second click on a row's compact button within this time sends nothing: a double click, or a
 * click before the agent's first output hid the button.
 */
export const COMPACT_REPEAT_MS = 3000;

/** The compact button was clicked `now`; `lastAt` is the previous send to that session. */
export function compactRepeated(lastAt: number | undefined, now: number): boolean {
  return lastAt !== undefined && now - lastAt < COMPACT_REPEAT_MS;
}

/**
 * The command starts over with an empty context (label keys `clear` and `new`), so its question
 * says what is lost; other commands with `confirm` get the question alone.
 */
export function startsOver(command: AgentCommand): boolean {
  return command.labelKey === "clear" || command.labelKey === "new";
}

/**
 * Types the command into one session the way "Send to several" does: a bracketed paste, then,
 * when the command submits, Enter as a separate write after `pause`. Resolves to `false` when
 * nothing was sent: a command with a line break or hidden characters never goes into a program
 * without bracketed paste (`blockedFor`).
 */
export async function sendCommand(
  target: BroadcastTarget,
  command: AgentCommand,
  pause: () => Promise<void>,
): Promise<boolean> {
  if (blockedFor(target, command.text)) return false;
  if (command.submit) {
    const blocked = await sendToTargets([target], command.text, pause);
    return blocked.length === 0;
  }
  await target.pasteAndWait(command.text);
  return true;
}

/** An entry of the commands submenu; the caller attaches what choosing it does. */
export type CommandMenuEntry =
  | { type: "heading"; key: string; label: string }
  | { type: "action"; key: string; label: string; detail: string; command: AgentCommand };

/**
 * The commands submenu of a session: its name as a caption, then one entry per command with its
 * text as the detail.
 */
export function commandMenuEntries(
  commands: readonly AgentCommand[],
  sessionTitle: string,
  t: Translate,
): CommandMenuEntry[] {
  return [
    {
      type: "heading",
      key: "heading",
      label: t("agentCommand.menuFor", { session: sessionTitle }),
    },
    ...commands.map((command): CommandMenuEntry => ({
      type: "action",
      key: `command:${command.text}`,
      label: commandLabel(command, t),
      detail: command.text,
      command,
    })),
  ];
}

/** Quick access entries for the active session's commands, found by label, text or program. */
export function commandLauncherItems(
  commands: readonly AgentCommand[],
  context: { sessionTitle: string; programName: string },
  t: Translate,
  run: (command: AgentCommand) => void,
): LauncherItem[] {
  return commands.map((command) => ({
    key: `agent-command:${command.text}`,
    section: "commands",
    label: commandLabel(command, t),
    detail: `${command.text} · ${context.sessionTitle}`,
    keywords: `${command.text} ${t("agentCommand.keyword")} slash command ${context.programName}`,
    run: () => {
      run(command);
    },
  }));
}
