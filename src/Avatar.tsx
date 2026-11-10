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
}: {
  name: string;
  /** Initials worked out elsewhere (say, from an e-mail address); `name` is ignored then. */
  text?: string;
  size?: "normal" | "small" | "mini";
}) {
  const letters = text ?? initials(name);
  return (
    <span className={`avatar ${size}`} aria-hidden="true">
      {letters === "" ? <PersonIcon size={size === "normal" ? 14 : 11} /> : letters}
    </span>
  );
}
