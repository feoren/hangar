import { useT } from "./i18n";
import { messageLines, type WaitingMessage } from "./partnerInbox";
import { Revealed } from "./Revealed";
import { useArmDelay } from "./useArmDelay";

/**
 * The messages agents of partner projects sent to this session's project (ADR 0012): sender and
 * text, and "Paste", "Paste and send", "Discard". Nothing reaches the terminal before a click.
 * The text is plain text with invisible characters shown as marked placeholders, line by line,
 * so it reads exactly as the agent will get it. Whenever the waiting messages change, every card
 * takes no click for a moment (useArmDelay.ts): a new card above or below can move the one under
 * the mouse, and a click meant for it must not answer another.
 */
export function WaitingMessages(props: {
  messages: readonly WaitingMessage[];
  /** Messages whose paste runs right now; their buttons wait. */
  busy: ReadonlySet<number>;
  /** Counts the changes of all waiting messages; each change arms every card anew. */
  generation: number;
  onPaste: (id: number, submit: boolean) => void;
  onDiscard: (id: number) => void;
}) {
  return (
    <div className="waiting-messages">
      {props.messages.map((message) => (
        <WaitingMessageCard
          key={message.id}
          message={message}
          busy={props.busy.has(message.id)}
          generation={props.generation}
          onPaste={props.onPaste}
          onDiscard={props.onDiscard}
        />
      ))}
    </div>
  );
}

function WaitingMessageCard({
  message,
  busy,
  generation,
  onPaste,
  onDiscard,
}: {
  message: WaitingMessage;
  busy: boolean;
  generation: number;
  onPaste: (id: number, submit: boolean) => void;
  onDiscard: (id: number) => void;
}) {
  const t = useT();
  const { armed, guard } = useArmDelay(`${String(message.id)}:${String(generation)}`);
  const title = t("inbox.from", { project: message.from });
  const button = (label: string, action: () => void, primary = false) => (
    <button
      type="button"
      className={primary ? "secondary-button waiting-message-send" : "secondary-button"}
      disabled={busy}
      aria-disabled={armed ? undefined : true}
      onClick={guard(action)}
    >
      {label}
    </button>
  );
  return (
    <section className="waiting-message" aria-label={title}>
      <div className="waiting-message-from">{title}</div>
      <div className="waiting-message-text">
        {messageLines(message.text).map((line, index) => (
          // Lines never move; the index is stable for this text.
          <div key={index}>{line === "" ? " " : <Revealed text={line} />}</div>
        ))}
      </div>
      <div className="waiting-message-actions">
        {button(t("partner.paste"), () => {
          onPaste(message.id, false);
        })}
        {button(
          t("partner.pasteSend"),
          () => {
            onPaste(message.id, true);
          },
          true,
        )}
        {button(t("inbox.discard"), () => {
          onDiscard(message.id);
        })}
      </div>
    </section>
  );
}
