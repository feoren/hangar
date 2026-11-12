import type { BadgeLook } from "./programPrefs";

/**
 * Program icon, a custom image or letters on a color; decorative, the name sits next to it.
 * `small`: the 14 px mark in front of the program name in a session's status line.
 */
export function Badge({ look, small = false }: { look: BadgeLook; small?: boolean }) {
  const size = small ? " small" : "";
  if (look.kind === "image") {
    const pixels = small ? 14 : 26;
    return (
      <span className={`badge badge-icon${size}`} aria-hidden={small ? true : undefined}>
        <img src={look.src} alt="" width={pixels} height={pixels} draggable={false} />
      </span>
    );
  }
  return (
    <span
      className={`${Array.from(look.text).length > 2 ? "badge wide" : "badge"}${size}`}
      style={{ background: look.color }}
      aria-hidden={small ? true : undefined}
    >
      {look.text}
    </span>
  );
}
