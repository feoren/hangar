import { commitMessage, type CommitMode } from "./gitActions";
import { useT } from "./i18n";

interface Props {
  message: string;
  onMessage: (message: string) => void;
  /** `commit` or `stageAll` (nothing staged); the box is not shown for `none`. */
  mode: Exclude<CommitMode, "none">;
  busy: boolean;
  onCommit: (message: string) => void;
  /** Agent sessions only: asks the agent in its terminal for a message. */
  onSuggest?: (() => void) | undefined;
}

/**
 * The commit box above the uncommitted changes: the message, "Commit" for the staged files or,
 * when nothing is staged, "Stage changed and commit" in so many words (tracked files only, new
 * files go in through their own box), and with an agent session a request to the agent for a
 * message (its answer appears in the terminal).
 */
export function CommitBox({ message, onMessage, mode, busy, onCommit, onSuggest }: Props) {
  const t = useT();
  const ready = commitMessage(message);
  return (
    <form
      className="commit-box"
      onSubmit={(event) => {
        event.preventDefault();
        if (ready !== null && !busy) onCommit(ready);
      }}
    >
      <textarea
        className="commit-message"
        rows={2}
        value={message}
        placeholder={t("changes.commitMessage")}
        aria-label={t("changes.commitMessage")}
        spellCheck={false}
        onChange={(event) => {
          onMessage(event.target.value);
        }}
      />
      <div className="commit-actions">
        {onSuggest && (
          <button
            type="button"
            className="link-button commit-suggest"
            title={t("changes.suggestHint")}
            disabled={busy}
            onClick={onSuggest}
          >
            {t("changes.suggest")}
          </button>
        )}
        <button
          type="submit"
          className="primary-button"
          title={mode === "stageAll" ? t("changes.stageAllHint") : undefined}
          disabled={busy || ready === null}
        >
          {mode === "commit" ? t("changes.commit") : t("changes.stageAllCommit")}
        </button>
      </div>
    </form>
  );
}
