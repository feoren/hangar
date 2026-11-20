import { useT, type Translate } from "./i18n";
import { LinkIcon } from "./icons";
import { linkColor, type LinkMark } from "./links";

/** What the mark says: the member's name, that messages wait while the brake holds, or that the
 * member leads the group. */
export function linkMarkText(mark: LinkMark, t: Translate): string {
  if (mark.braked) return t("links.brakedMarker", { name: mark.name });
  return mark.lead
    ? t("links.leadMarker", { name: mark.name })
    : t("links.marker", { name: mark.name });
}

/**
 * The chain on a member's row (ADR 0014) in its group's color, so the linked sessions are easy to
 * spot; while the loop brake holds, it shows a ring as well, shape and not color alone. The lead's
 * chain carries a small dot. `rail`:
 * the small form at the badge's corner in the collapsed sidebar, whose button names it already.
 */
export function LinkMarkSign({ mark, rail = false }: { mark: LinkMark; rail?: boolean }) {
  const t = useT();
  const text = linkMarkText(mark, t);
  const className = [
    rail ? "rail-link" : "session-link",
    mark.braked ? "braked" : "",
    mark.lead ? "lead" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <span
      className={className}
      style={{ color: linkColor(mark.color) }}
      title={rail ? undefined : text}
      aria-hidden={rail ? "true" : undefined}
      data-link-group={mark.group}
    >
      <LinkIcon size={rail ? 13 : 12} />
      {!rail && <span className="sr-only">{text}</span>}
    </span>
  );
}
