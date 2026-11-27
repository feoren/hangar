// The models of a program row in "New session" (ADR 0022): a flyout beside the row with the
// program's own models and the models of the local servers that run. It opens when the pointer
// rests on the row or with Right arrow (ProgramRow.tsx); a click starts the session with that
// model, while the row itself keeps starting with the program's default. The flyout is fixed to the
// window (PopupMenu), since the group's body scrolls.

import type { RefObject } from "react";

import type { SessionModel } from "./api";
import { useT, type Translate } from "./i18n";
import type { ModelSection } from "./localLlm";
import { PopupMenu, type PopupItem } from "./PopupMenu";

/** The menu's rows: a heading per section, then its models; loaded local models say so quietly. */
export function modelMenuItems(
  program: string,
  sections: readonly ModelSection[],
  onPick: (model: SessionModel) => void,
  t: Translate,
): PopupItem[] {
  return sections.flatMap((section): PopupItem[] => [
    { type: "heading", key: `heading:${section.source}`, label: section.heading },
    ...section.entries.map((entry): PopupItem => ({
      type: "action",
      key: `${section.source}:${entry.model.id}`,
      label: entry.label,
      ...(entry.loaded ? { detail: t("model.loaded") } : {}),
      title: t("model.startWith", { program, model: entry.label }),
      run: () => {
        onPick(entry.model);
      },
    })),
  ]);
}

/** Gap between a row and its flyout. */
const FLYOUT_GAP = 4;

/** A flyout's top left corner, and the y its bottom goes to when it does not fit below. */
export interface FlyoutPlace {
  x: number;
  y: number;
  flipAbove: number;
}

/**
 * Where a flyout beside a row goes: right of it and level with its top; where the window ends
 * below, it ends at the row's bottom instead (PopupMenu then keeps it inside the window).
 */
export function flyoutPlace(row: { right: number; top: number; bottom: number }): FlyoutPlace {
  return { x: row.right + FLYOUT_GAP, y: row.top, flipAbove: row.bottom };
}

export function ModelFlyout(props: {
  program: string;
  sections: readonly ModelSection[];
  place: FlyoutPlace;
  /** Opened with the keyboard: the first model takes the focus. */
  autoFocus: boolean;
  /** The row: clicking it does not count as a click outside. */
  anchor: RefObject<HTMLElement | null>;
  onPick: (model: SessionModel) => void;
  /** Left arrow or Escape: back to the row. */
  onBack: () => void;
  /** A model was picked (`action`) or the flyout was dismissed (Tab, click outside, ...). */
  onClose: (reason: "action" | "dismiss") => void;
}) {
  const t = useT();
  return (
    <PopupMenu
      items={modelMenuItems(props.program, props.sections, props.onPick, t)}
      label={t("model.menuNamed", { name: props.program })}
      position={props.place}
      flipAbove={props.place.flipAbove}
      anchor={props.anchor}
      className="model-menu"
      autoFocus={props.autoFocus}
      onBack={props.onBack}
      onClose={props.onClose}
    />
  );
}
