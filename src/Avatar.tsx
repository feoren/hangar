import { PersonIcon } from "./icons";
import { initials, type Presence } from "./teamOverview";

/**
 * A round avatar: the person's picture when there is one, else their initials, else a person
 * icon. Decorative: the name always stands next to it or in the accessible name of its button.
 */
export function Avatar({
  name,
  text,
  src = null,
  size = "normal",
  session = false,
}: {
  name: string;
  /** Initials worked out elsewhere (say, from an e-mail address); `name` is ignored then. */
  text?: string;
  /**
   * The picture as a `data:` URL from the backend (`Me.avatar`); `null` or missing shows the
   * initials. Never a remote address: the window's CSP allows none.
   */
  src?: string | null;
  size?: "normal" | "small" | "mini";
  /** One of the user's own sessions, not a person: a rounded square instead of a circle. */
  session?: boolean;
}) {
  const shape = session ? `avatar ${size} session` : `avatar ${size}`;
  if (src !== null && src !== "") {
    return (
      <span className={`${shape} picture`} aria-hidden="true">
        <img src={src} alt="" draggable={false} />
      </span>
    );
  }
  const letters = text ?? initials(name);
  return (
    <span className={shape} aria-hidden="true">
      {letters === "" ? <PersonIcon size={size === "normal" ? 14 : 11} /> : letters}
    </span>
  );
}

/**
 * A teammate: their avatar with one presence dot, the same in the Team window, the sidebar and
 * the session rows. Here is a filled dot, away a hollow ring, so the shape tells it apart as well
 * as the color. Decorative like the avatar: the words stand next to it.
 */
export function PersonAvatar(props: {
  name: string;
  presence: Presence;
  size?: "normal" | "small" | "mini";
}) {
  return (
    <span className={`person-avatar ${props.size ?? "normal"}`} aria-hidden="true">
      <Avatar name={props.name} size={props.size ?? "normal"} />
      <span className={`presence-dot ${props.presence}`} />
    </span>
  );
}
