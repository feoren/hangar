import { useState } from "react";

import { folderKey } from "./folders";

import { folderMenuSections, type FolderSectionKind } from "./folderSections";
import { isFavorite, type FolderPrefs } from "./folders";
import { useT, type MessageKey } from "./i18n";
import { CheckIcon, FolderIcon, StarIcon } from "./icons";
import { PopupMenu, type PopupItem } from "./PopupMenu";

interface Props {
  /** Where the next session starts. */
  target: string | null;
  /** Project folders of the running sessions. */
  running: readonly string[];
  folders: FolderPrefs;
  position: { x: number; y: number };
  /** Top edge of the opening button, so the menu can open above it (PopupMenu). */
  flipAbove?: number;
  anchor?: React.RefObject<HTMLElement | null>;
  onSelect: (path: string) => void;
  /** Opens the system's folder dialog. */
  onChoose: () => void;
  onToggleFavorite: (path: string) => void;
  /** Takes a favorite or recent folder out of the menu (folders.ts `forgetFolder`). */
  onForget: (path: string) => void;
  onClose: (reason: "action" | "dismiss") => void;
}

const SECTION_TITLES: Record<FolderSectionKind, MessageKey> = {
  running: "folders.running",
  favorites: "folders.favorites",
  recent: "folders.recent",
};

/**
 * Folder menu: running projects, favorites and recent folders, each row with a star to keep or
 * drop it as a favorite and, for saved folders, an x to take it out of the menu; then the folder
 * dialog. Used by the sidebar, the rail and the empty state.
 */
export function FolderMenu(props: Props) {
  const t = useT();
  // The rows stay where they are while the menu is open; a star only changes its own state.
  const [frozen] = useState(() => ({ running: props.running, prefs: props.folders }));
  /** Folders taken out while the menu is open: their rows go, the others stay in place. */
  const [forgotten, setForgotten] = useState<ReadonlySet<string>>(new Set());
  const kept = (paths: readonly string[]) =>
    paths.filter((path) => !forgotten.has(folderKey(path)));
  const sections = folderMenuSections({
    running: frozen.running,
    prefs: { favorites: kept(frozen.prefs.favorites), recent: kept(frozen.prefs.recent) },
    target: props.target,
  });

  const items: PopupItem[] = [];
  for (const section of sections) {
    items.push({
      type: "heading",
      key: `h:${section.kind}`,
      label: t(SECTION_TITLES[section.kind]),
    });
    for (const entry of section.entries) {
      const favorite = isFavorite(props.folders, entry.path);
      const detail = entry.current ? t("folders.current") : entry.detail;
      items.push({
        type: "action",
        key: `${section.kind}:${entry.path}`,
        label: entry.label,
        ...(detail === "" ? {} : { detail }),
        title: entry.path,
        current: entry.current,
        icon: entry.current ? <CheckIcon size={14} /> : <FolderIcon size={14} />,
        secondary: {
          // The star shows the state: filled is a favorite, the label names what a click does.
          label: favorite
            ? t("folders.removeFavoriteNamed", { folder: entry.label })
            : t("folders.addFavoriteNamed", { folder: entry.label }),
          icon: <StarIcon filled={favorite} size={14} />,
          checked: favorite,
          keepOpen: true,
          run: () => {
            props.onToggleFavorite(entry.path);
          },
        },
        // Running projects come from the sessions, so only saved folders can be taken out.
        ...(section.kind === "running"
          ? {}
          : {
              remove: {
                label: t("folders.forgetNamed", { folder: entry.label }),
                run: () => {
                  setForgotten((current) => new Set(current).add(folderKey(entry.path)));
                  props.onForget(entry.path);
                },
              },
            }),
        run: () => {
          props.onSelect(entry.path);
        },
      });
    }
  }
  if (items.length > 0) items.push({ type: "separator", key: "sep" });
  items.push({
    type: "action",
    key: "choose",
    label: t("folders.chooseOther"),
    icon: <FolderIcon size={14} />,
    run: props.onChoose,
  });

  return (
    <PopupMenu
      items={items}
      label={t("folders.menu")}
      position={props.position}
      className="folder-menu"
      {...(props.flipAbove === undefined ? {} : { flipAbove: props.flipAbove })}
      {...(props.anchor === undefined ? {} : { anchor: props.anchor })}
      onClose={props.onClose}
    />
  );
}
