// The bracket that marks a link group in the sidebar (ADR 0014): a thin line in the group's color
// beside every run of member rows that follow each other, with a label above the group's first
// run. Members can sit in different sidebar groups or be split by other rows; each stretch of
// neighbours gets its own piece of line, all in the same color. Pure, so the runs are tested
// without React.

import type { LinkMark } from "./links";

/** A row as the list shows it: its session and the sidebar group (program or project) it is in. */
export interface BracketRow {
  id: number;
  section: string;
}

/** How one member row takes part in its group's bracket. */
export interface BracketPiece {
  group: number;
  /** Index into the link palette (links.ts `linkColor`). */
  color: number;
  /** The loop brake holds the group's messages: the line is drawn dashed. */
  braked: boolean;
  /** The first row of its run: the line starts here. */
  first: boolean;
  /** The last row of its run: the line ends here. */
  last: boolean;
  /** The first row of the group's first run in the list: the label stands above it. */
  label: boolean;
  /** How many sessions the whole group has, shown or not. */
  members: number;
}

/**
 * The bracket piece of every member row in `rows`, in list order. A run ends where the next row
 * is no member of the same group or sits in another sidebar group.
 */
export function linkBrackets(
  rows: readonly BracketRow[],
  marks: ReadonlyMap<number, LinkMark>,
): Map<number, BracketPiece> {
  const members = new Map<number, number>();
  for (const mark of marks.values()) members.set(mark.group, (members.get(mark.group) ?? 0) + 1);
  const labelled = new Set<number>();
  const pieces = new Map<number, BracketPiece>();
  rows.forEach((row, index) => {
    const mark = marks.get(row.id);
    if (mark === undefined) return;
    const sameRun = (other: BracketRow | undefined) =>
      other !== undefined &&
      other.section === row.section &&
      marks.get(other.id)?.group === mark.group;
    const first = !sameRun(rows[index - 1]);
    const label = first && !labelled.has(mark.group);
    if (label) labelled.add(mark.group);
    pieces.set(row.id, {
      group: mark.group,
      color: mark.color,
      braked: mark.braked,
      first,
      last: !sameRun(rows[index + 1]),
      label,
      members: members.get(mark.group) ?? 0,
    });
  });
  return pieces;
}
