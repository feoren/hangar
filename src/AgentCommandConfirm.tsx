// The question before an agent command with `confirm` (such as `/clear`) is typed into a session.
// Such a command usually throws the conversation away, so Cancel has the focus and Send is the
// danger button; the sentence about the lost context shows only for commands that start over.

import { startsOver } from "./agentCommands";
import type { AgentCommand } from "./api";
import { Dialog } from "./Dialog";
import { useT } from "./i18n";

interface Props {
  command: AgentCommand;
  sessionTitle: string;
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentCommandConfirm({ command, sessionTitle, onCancel, onConfirm }: Props) {
  const t = useT();
  return (
    <Dialog labelledBy="agent-command-title" onClose={onCancel}>
      <h2 id="agent-command-title">
        {t("agentCommand.confirm", { command: command.text, session: sessionTitle })}
      </h2>
      {startsOver(command) && <p className="muted">{t("agentCommand.confirmBody")}</p>}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" data-autofocus onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button danger" onClick={onConfirm}>
          {t("agentCommand.send")}
        </button>
      </div>
    </Dialog>
  );
}
