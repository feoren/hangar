import { useId, useLayoutEffect, useRef, useState } from "react";

import { Badge } from "./Badge";
import { useT } from "./i18n";
import { ChevronIcon, CloseIcon } from "./icons";
import type { IntroSummary } from "./linkIntro";
import { cardPreview, cardTime, type CardSend } from "./messageCard";
import type { WaitingMessage } from "./partnerInbox";
import type { BadgeLook } from "./programPrefs";
import { Revealed } from "./Revealed";
import { secretLabels } from "./secretScan";
import { useArmDelay } from "./useArmDelay";

/**
 * The messages that wait below a session row for the person's click: from agents of partner
 * projects (ADR 0012) and from members of its link group (ADR 0014). Sender and time in one line,
 * the text cut to a few lines with "More", one clear "Send" (paste plus Enter), "Paste only" as a
 * quiet text button and discard as a close mark. Nothing reaches the terminal before a click.
 *
 * While a send waits for its target to pause, the card says so with "Cancel" instead of its
 * buttons, and goes once the message is in. The text is plain text with invisible characters
 * shown as marked placeholders, line by line, so it reads exactly as the agent will get it.
 * Whenever the cards change, every card takes no click for a moment (useArmDelay.ts): a new card
 * above or below can move the one under the mouse, and a click meant for it must not answer
 * another. A message that seems to hold a secret key says so above its text (ADR 0015).
 *
 * Hangar's own introduction of a link group is written for the agent; its card shows people
 * whom the session now works with instead, and the agent's text only on request. While a send
 * waits, the status names no session in its sentence: the card sits below its target, and where
 * it does not (a partner message shown on several rows), the target follows as a label.
 *
 * The head folds a card to its one line (chevron and sender). A card folds by itself once its
 * send waits, since nothing is left to decide but "Cancel"; a click on the head opens it again.
 * A secret hint stays visible on a folded card.
 */
export function WaitingMessages(props: {
  messages: readonly WaitingMessage[];
  /** Sends that run from a card right now, by message id (messageCard.ts `CardSends`). */
  sends: ReadonlyMap<number, CardSend>;
  /** Changes whenever any card appears or goes; each change arms every card anew. */
  generation: string | number;
  /** UI language, for the time. */
  language: string;
  /** The session whose row the cards sit below. */
  session?: number;
  /** A program's icon, for the members of an introduction. */
  badgeFor?: (profileId: string) => BadgeLook;
  onSend: (id: number, submit: boolean) => void;
  onCancel: (id: number) => void;
  onDiscard: (id: number) => void;
}) {
  return (
    <div className="waiting-messages">
      {props.messages.map((message) => (
        <WaitingMessageCard
          key={message.id}
          message={message}
          send={props.sends.get(message.id)}
          generation={props.generation}
          language={props.language}
          session={props.session}
          badgeFor={props.badgeFor}
          onSend={props.onSend}
          onCancel={props.onCancel}
          onDiscard={props.onDiscard}
        />
      ))}
    </div>
  );
}

function WaitingMessageCard({
  message,
  send,
  generation,
  language,
  session,
  badgeFor,
  onSend,
  onCancel,
  onDiscard,
}: {
  message: WaitingMessage;
  send: CardSend | undefined;
  generation: string | number;
  language: string;
  session: number | undefined;
  badgeFor: ((profileId: string) => BadgeLook) | undefined;
  onSend: (id: number, submit: boolean) => void;
  onCancel: (id: number) => void;
  onDiscard: (id: number) => void;
}) {
  const t = useT();
  const textId = useId();
  const [open, setOpen] = useState(false);
  // Today or not is told once, when the card appears: a card rarely waits past midnight.
  const [shownAt] = useState(() => Date.now());
  const { armed, guard } = useArmDelay(`${String(message.id)}:${String(generation)}`);
  const title = message.title ?? t("inbox.from", { project: message.from });
  const intro = message.intro;
  const preview = cardPreview(message.text);
  const lines =
    intro !== undefined || open || !preview.cut ? message.text.split("\n") : preview.lines;
  // The target is named only where the card is not below it.
  const elsewhere = send?.to !== undefined && send.to !== session ? send.target : null;
  const waitArm = armed ? undefined : true;
  const discard = t("inbox.discard");
  const card = useRef<HTMLElement>(null);
  const sendButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const headButton = useRef<HTMLButtonElement>(null);
  /** The keyboard focus was in this card last; a button that goes takes it to the page. */
  const focusInside = useRef(false);
  const mode = send === undefined ? "idle" : send.phase;
  const [folded, setFolded] = useState(mode === "waiting");
  const status =
    send === undefined
      ? ""
      : send.phase === "waiting"
        ? t(send.submit ? "inbox.waitSend" : "inbox.waitPaste")
        : t(send.submit ? "inbox.sending" : "inbox.pasting");
  const bodyId = useId();
  // A send that starts waiting folds the card, one that ends opens it; the person may toggle it
  // in between. Adjusted during render, not in an effect, so the card never shows a wrong frame.
  const [foldedFor, setFoldedFor] = useState(mode);
  if (foldedFor !== mode) {
    setFoldedFor(mode);
    if (mode === "waiting") setFolded(true);
    else if (mode === "idle") setFolded(false);
  }
  // The buttons swap when a send starts or ends. Focus that was in the card follows: to "Cancel"
  // while the send waits, back to "Send" when it ended and the card stays (cancelled, failed).
  // Focus the person moved elsewhere stays there.
  useLayoutEffect(() => {
    const active = document.activeElement;
    const lost = active === null || active === document.body;
    if (!focusInside.current || !(lost || card.current?.contains(active) === true)) return;
    if (mode === "waiting") headButton.current?.focus();
    else if (mode === "idle") sendButton.current?.focus();
  }, [mode]);
  return (
    // No aria-busy: it would keep screen readers from announcing the status line inside.
    <section
      ref={card}
      className="waiting-message"
      aria-label={title}
      data-sending={send !== undefined}
      data-folded={folded}
      onFocus={() => {
        focusInside.current = true;
      }}
      onBlur={(event) => {
        // A removed button blurs without a new target: the focus counts as still here.
        const next = event.relatedTarget;
        if (next !== null && !event.currentTarget.contains(next)) focusInside.current = false;
      }}
    >
      <div className="waiting-message-head">
        <button
          ref={headButton}
          type="button"
          className="waiting-message-fold"
          aria-expanded={!folded}
          aria-controls={folded ? undefined : bodyId}
          title={status === "" ? undefined : status}
          onClick={() => {
            setFolded((current) => !current);
          }}
        >
          <ChevronIcon size={12} />
          <span className="waiting-message-from">{title}</span>
        </button>
        <time className="waiting-message-time" dateTime={new Date(message.at).toISOString()}>
          {cardTime(message.at, shownAt, language)}
        </time>
        {send === undefined && (
          <button
            type="button"
            className="icon-button waiting-message-discard"
            aria-label={discard}
            title={discard}
            aria-disabled={waitArm}
            onClick={guard(() => {
              onDiscard(message.id);
            })}
          >
            <CloseIcon />
          </button>
        )}
      </div>
      {message.secrets !== undefined && message.secrets.length > 0 && (
        <div className="waiting-message-secret" role="note">
          {t("message.secretHint", { kinds: secretLabels(message.secrets, t) })}
        </div>
      )}
      {/* Always there, empty while nothing runs: a live region only announces changes to a
          region that already existed. Outside the body, so a folded card still announces. */}
      <span className="sr-only" role="status">
        {status}
      </span>
      <div className="waiting-message-body" id={bodyId} hidden={folded}>
        {intro !== undefined && <IntroMembers intro={intro} badgeFor={badgeFor} />}
        {(intro === undefined || open) && (
          <div className="waiting-message-text" id={textId}>
            {lines.map((line, index) => (
              // Lines never move; the index is stable for this text.
              <div key={index}>{line === "" ? " " : <Revealed text={line} />}</div>
            ))}
          </div>
        )}
        {intro !== undefined && (
          <button
            type="button"
            className="link-button waiting-message-more"
            aria-expanded={open}
            aria-controls={open ? textId : undefined}
            onClick={() => {
              setOpen((current) => !current);
            }}
          >
            {t(open ? "links.introHide" : "links.introShow")}
          </button>
        )}
        {intro === undefined && preview.cut && (
          <button
            type="button"
            className="link-button waiting-message-more"
            aria-expanded={open}
            aria-controls={textId}
            onClick={() => {
              setOpen((current) => !current);
            }}
          >
            {t(open ? "inbox.less" : "inbox.more")}
          </button>
        )}
        <div className="waiting-message-actions">
          {send === undefined && (
            <button
              ref={sendButton}
              type="button"
              className="primary-button waiting-message-send"
              aria-disabled={waitArm}
              onClick={guard(() => {
                onSend(message.id, true);
              })}
            >
              {t("inbox.send")}
            </button>
          )}
          {send === undefined && (
            <button
              type="button"
              className="link-button"
              aria-disabled={waitArm}
              onClick={guard(() => {
                onSend(message.id, false);
              })}
            >
              {t("inbox.pasteOnly")}
            </button>
          )}
          <span className="waiting-message-status" aria-hidden="true">
            {status}
          </span>
          {elsewhere !== null && (
            <span className="waiting-message-target" title={elsewhere}>
              {t("inbox.toSession", { session: elsewhere })}
            </span>
          )}
          {send?.phase === "waiting" && (
            <button
              ref={cancelButton}
              type="button"
              className="link-button"
              onClick={() => {
                onCancel(message.id);
              }}
            >
              {t("common.cancel")}
            </button>
          )}
        </div>
      </div>
    </section>
  );
}

/** Whom the session works with now: one line, then each other member as a chip. */
function IntroMembers({
  intro,
  badgeFor,
}: {
  intro: IntroSummary;
  badgeFor: ((profileId: string) => BadgeLook) | undefined;
}) {
  const t = useT();
  const lead = t("links.lead");
  return (
    <div className="link-intro">
      <div className="link-intro-lede">
        {t(intro.leads ? "links.introLeads" : "links.introWith")}
      </div>
      <ul className="link-intro-members">
        {intro.members.map((member) => (
          <li
            key={member.session}
            className="link-intro-member"
            title={member.lead ? `${member.detail} · ${lead}` : member.detail}
          >
            {badgeFor !== undefined && member.profileId !== "" && (
              // Letters like "CC" would be read out; the name next to it says who it is.
              <span className="link-intro-badge" aria-hidden="true">
                <Badge look={badgeFor(member.profileId)} />
              </span>
            )}
            <span className="link-intro-name">{member.label}</span>
            {member.lead && <span className="link-intro-lead">{lead}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}
