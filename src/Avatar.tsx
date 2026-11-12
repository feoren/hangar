import { PersonIcon } from "./icons";
import { initials } from "./teamOverview";

/**
 * A round avatar with a person's initials, or a person icon when the name has none. Decorative:
 * the name always stands next to it or in the accessible name of its button.
 */
export function Avatar({
  name,
  text,
  size = "normal",
  session = false,
}: {
  name: string;
  /** Initials worked out elsewhere (say, from an e-mail address); `name` is ignored then. */
  text?: string;
  size?: "normal" | "small" | "mini";
  /** One of the user's own sessions, not a person: a rounded square instead of a circle. */
  session?: boolean;
}) {
  const letters = text ?? initials(name);
  return (
    <span className={session ? `avatar ${size} session` : `avatar ${size}`} aria-hidden="true">
      {letters === "" ? <PersonIcon size={size === "normal" ? 14 : 11} /> : letters}
    </span>
  );
}
