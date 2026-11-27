// The quiet "Model" pill at the end of a program row in "New session" (ADR 0022): it opens a menu
// next to the row with the program's own models and the models of the local servers that run. A
// click starts the session with that model; the row itself keeps starting with the program's
// default. A word instead of a symbol, so nobody has to guess what it does. It is a button of its
// own beside the row's button (no button in a button). The menu is fixed to the window
// (PopupMenu), since the group's body scrolls.

import { useRef, useState } from "react";

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

export function ModelMenuButton(props: {
  program: string;
  sections: readonly ModelSection[];
  onPick: (model: SessionModel) => void;
  /** A model was picked: the caller decides where the focus goes. */
  onAction: () => void;
}) {
  const t = useT();
  const button = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; flipAbove: number } | null>(null);
  const label = t("model.menuNamed", { name: props.program });

  const open = (target: HTMLElement) => {
    // Beside the whole row, level with its top, like a submenu.
    const row = target.closest(".profile-row") ?? target;
    const rect = row.getBoundingClientRect();
    setMenu({ x: rect.right + 4, y: rect.top, flipAbove: rect.bottom });
  };

  return (
    <>
      <button
        ref={button}
        type="button"
        className="profile-models"
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        onClick={(event) => {
          if (menu !== null) setMenu(null);
          else open(event.currentTarget);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight" && menu === null) {
            event.preventDefault();
            open(event.currentTarget);
          }
        }}
      >
        <span className="profile-models-pill">{t("model.pill")}</span>
      </button>
      {menu !== null && (
        <PopupMenu
          items={modelMenuItems(props.program, props.sections, props.onPick, t)}
          label={label}
          position={menu}
          flipAbove={menu.flipAbove}
          anchor={button}
          className="model-menu"
          onClose={(reason) => {
            setMenu(null);
            if (reason === "dismiss") button.current?.focus();
            else props.onAction();
          }}
        />
      )}
    </>
  );
}
