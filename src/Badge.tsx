import type { BadgeLook } from "./programPrefs";

/** Program icon, a custom image or letters on a color; decorative, the name sits next to it. */
export function Badge({ look }: { look: BadgeLook }) {
  if (look.kind === "image") {
    return (
      <span className="badge badge-icon">
        <img src={look.src} alt="" width={26} height={26} draggable={false} />
      </span>
    );
  }
  return (
    <span className="badge" style={{ background: look.color }}>
      {look.text}
    </span>
  );
}
