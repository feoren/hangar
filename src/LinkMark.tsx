import { useId, type CSSProperties } from "react";

import { useT, type Translate } from "./i18n";
import { CrownIcon } from "./icons";
import type { BracketPiece } from "./linkBracket";
import type { PendingDelivery } from "./linkCards";
import { historyPreview, linkColor, type LinkMark } from "./links";

/** What a member row says about its link: the member's name, that messages wait while the brake
 * holds, or that the member leads the group. */
export function linkMarkText(mark: LinkMark, t: Translate): string {
  if (mark.braked) return t("links.brakedMarker", { name: mark.name });
  return mark.lead
    ? t("links.leadMarker", { name: mark.name })
    : t("links.marker", { name: mark.name });
}

type BracketStyle = CSSProperties & { "--link-color": string };

/** The group's color for the bracket's line, label and crown (styles.css "Link bracket"). */
function bracketStyle(color: number): BracketStyle {
  return { "--link-color": linkColor(color) };
}

/**
 * The bracket's piece of line beside one item of the list (ADR 0014): a member row, or an item
 * that hangs below one inside the run (waiting messages, the sub-agent tree). Decoration only;
 * the row itself names its link for screen readers.
 */
export function LinkBracketLine({
  piece,
  between = false,
}: {
  piece: BracketPiece;
  /** An item below a member row, inside the run: the line only passes by. */
  between?: boolean;
}) {
  const className = [
    "link-bracket",
    between ? "between" : "",
    !between && piece.first && !piece.label ? "start" : "",
    !between && piece.last ? "end" : "",
    piece.braked ? "braked" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return <span className={className} style={bracketStyle(piece.color)} aria-hidden="true" />;
}

/** Characters of a message's first line in the tooltip of the messages on their way. */
const PENDING_PREVIEW_CHARS = 60;

/** One line per message on its way, for the label's tooltip: "app to web: first line". */
export function pendingLines(pending: readonly PendingDelivery[], t: Translate): string[] {
  return pending.map((entry) => {
    const route = t("links.historyRoute", {
      from: entry.fromName ?? t("links.hangar"),
      to: entry.toName,
    });
    const line = historyPreview(entry.text);
    const chars = Array.from(line);
    const short =
      chars.length > PENDING_PREVIEW_CHARS
        ? `${chars.slice(0, PENDING_PREVIEW_CHARS - 1).join("")}…`
        : line;
    return short === "" ? route : `${route}: ${short}`;
  });
}

/**
 * The label above a group's first run: "Group · 3", in the group's color, and "2 on the way"
 * while messages go on their own; the tooltip lists them. A click opens the group's menu
 * (lead, brake, history, unlink). While the loop brake holds, the line is dashed and the label
 * says why. The label stands in its own row, so none of this takes width from a session row.
 */
export function LinkBracketHead({
  piece,
  pending,
  menuOpen,
  onMenu,
}: {
  piece: BracketPiece;
  /** The group's messages on their way (linkCards.ts `pendingByGroup`). */
  pending: readonly PendingDelivery[];
  menuOpen: boolean;
  onMenu: (group: number, anchor: HTMLElement) => void;
}) {
  const t = useT();
  const listId = useId();
  const group = t("links.bracketMenu", { count: piece.members });
  const name = piece.braked ? `${group}, ${t("links.bracketBraked")}` : group;
  const lines = pendingLines(pending, t);
  const count = lines.length > 0 ? t("links.pending", { count: lines.length }) : null;
  const tooltip = [name, ...lines].join("\n");
  // The visible label starts the button's name, so voice control finds it by what it shows.
  return (
    <li className="link-bracket-head" style={bracketStyle(piece.color)}>
      <span
        className={piece.braked ? "link-bracket head braked" : "link-bracket head"}
        aria-hidden="true"
      />
      <button
        type="button"
        className="link-bracket-label"
        title={tooltip}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        aria-describedby={lines.length > 0 ? listId : undefined}
        onClick={(event) => {
          onMenu(piece.group, event.currentTarget);
        }}
      >
        {t("links.bracket", { count: piece.members })}
        {count !== null && <span className="link-bracket-pending">{count}</span>}
        <span className="sr-only">: {name}</span>
      </button>
      {lines.length > 0 && (
        <span id={listId} className="sr-only">
          {lines.join("; ")}
        </span>
      )}
    </li>
  );
}

/** The collapsed sidebar's form: a colored edge beside the badge, run by run. */
export function LinkRailEdge({ piece }: { piece: BracketPiece }) {
  const className = [
    "rail-link-edge",
    piece.first ? "start" : "",
    piece.last ? "end" : "",
    piece.braked ? "braked" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return <span className={className} style={bracketStyle(piece.color)} aria-hidden="true" />;
}

/** The small crown before the lead's title; its row names the lead for screen readers. */
export function LeadCrown({ color }: { color: number }) {
  const t = useT();
  return (
    <span
      className="session-crown"
      style={bracketStyle(color)}
      title={t("links.leads")}
      aria-hidden="true"
    >
      <CrownIcon size={11} />
    </span>
  );
}
