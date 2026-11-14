// The compact button in a session row's actions strip: from the context meter's warning share on
// it types the program's compact command (`/compact`, `/compress`) into the session. It lives in
// the actions strip, not in the facts line, so it takes no room in the row's columns, and it
// re-renders only with this session's context numbers (useSessionContext). Hidden while the agent
// is busy, as Hangar offers only what works right now (agentCommands.ts `showCompact`).

import type { AgentCommand } from "./api";
import { compactCommand, commandLabel, showCompact } from "./agentCommands";
import { contextShare } from "./contextUsage";
import { useT } from "./i18n";
import { CompactIcon } from "./icons";
import { useSessionContext } from "./useContextUsage";

interface Props {
  sessionId: number;
  /** The session's commands right now (agentCommands.ts `sessionCommands`). */
  commands: readonly AgentCommand[];
  /** The agent works or waits for an answer (agentCommands.ts `commandBusy`). */
  busy: boolean;
  /** `at`: the click's time stamp (performance.now() clock), for the repeated-click check. */
  onRun: (sessionId: number, command: AgentCommand, at: number) => void;
}

export function CompactAction({ sessionId, commands, busy, onRun }: Props) {
  const t = useT();
  const context = useSessionContext(sessionId);
  const share = context === undefined ? null : contextShare(context);
  const command = compactCommand(commands);
  if (command === null || !showCompact(share, commands, busy)) return null;
  const label = commandLabel(command, t);
  return (
    <button
      type="button"
      className="icon-button session-compact"
      aria-label={label}
      title={`${label} (${command.text})`}
      onClick={(event) => {
        onRun(sessionId, command, event.timeStamp);
      }}
    >
      <CompactIcon />
    </button>
  );
}
