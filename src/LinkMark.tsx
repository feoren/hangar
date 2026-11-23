import type { CSSProperties } from "react";

import { useT, type Translate } from "./i18n";
import { CrownIcon } from "./icons";
import type { BracketPiece } from "./linkBracket";
import { linkColor, type LinkMark } from "./links";

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

/**
 * The label above a group's first run: "Group · 3", in the group's color; a click opens Edit
 * group. While the loop brake holds, the line is dashed and the label says why.
 */
export function LinkBracketHead({
  piece,
  onEdit,
}: {
  piece: BracketPiece;
  onEdit: (group: number) => void;
}) {
  const t = useT();
  const edit = t("links.bracketEdit", { count: piece.members });
  const name = piece.braked ? `${edit}, ${t("links.bracketBraked")}` : edit;
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
        title={name}
        onClick={() => {
          onEdit(piece.group);
        }}
      >
        {t("links.bracket", { count: piece.members })}
        <span className="sr-only">: {name}</span>
      </button>
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
