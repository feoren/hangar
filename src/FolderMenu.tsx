import { useState } from "react";

import { folderMenuSections, type FolderSectionKind } from "./folderSections";
import {
  folderColor,
  folderColorCss,
  folderKey,
  isFavorite,
  moveFavorite,
  type FolderColor,
  type FolderPrefs,
} from "./folders";
import { useT, type MessageKey } from "./i18n";
import { CheckIcon, FolderIcon, StarIcon } from "./icons";
import { MARKER_COLOR_NAMES, MARKER_COLORS } from "./marker";
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
  /** Puts the favorite `path` right before (or after) the favorite `target` (`moveFavorite`). */
  onMoveFavorite: (path: string, target: string, after: boolean) => void;
  /** Gives a project folder a color; `null` takes it away. */
  onColor: (path: string, color: FolderColor | null) => void;
  onClose: (reason: "action" | "dismiss") => void;
}

const SECTION_TITLES: Record<FolderSectionKind, MessageKey> = {
  running: "folders.running",
  favorites: "folders.favorites",
  recent: "folders.recent",
};

/** Swatch key for "no color". */
const NO_COLOR = "none";

/**
 * Folder menu: running projects, favorites and recent folders, each row with a star to keep or
 * drop it as a favorite, a dot for the folder's color and, for saved folders, an x to take it
 * out of the menu; then the folder dialog. Favorites can be dragged into another order or moved
 * with Alt+Up and Alt+Down. Used by the sidebar and the rail.
 */
export function FolderMenu(props: Props) {
  const t = useT();
  // The rows stay where they are while the menu is open; a star only changes its own state.
  const [frozen] = useState(() => ({ running: props.running, prefs: props.folders }));
  /** Favorites in the order the person arranged them while the menu is open. */
  const [favorites, setFavorites] = useState<readonly string[]>(frozen.prefs.favorites);
  /** Folders taken out while the menu is open: their rows go, the others stay in place. */
  const [forgotten, setForgotten] = useState<ReadonlySet<string>>(new Set());
  /** Entry whose color swatches are open below it. */
  const [coloring, setColoring] = useState<string | null>(null);
  const kept = (paths: readonly string[]) =>
    paths.filter((path) => !forgotten.has(folderKey(path)));
  const sections = folderMenuSections({
    running: frozen.running,
    prefs: { favorites: kept(favorites), recent: kept(frozen.prefs.recent) },
    target: props.target,
  });
  const moveHint = t("session.moveHint");

  /** Moves a favorite next to another one, in the menu at once and in the stored list. */
  const move = (path: string, target: string, after: boolean) => {
    setFavorites(
      (current) =>
        moveFavorite({ favorites: current, recent: [], colors: {} }, path, target, after).favorites,
    );
    props.onMoveFavorite(path, target, after);
  };

  const items: PopupItem[] = [];
  for (const section of sections) {
    items.push({
      type: "heading",
      key: `h:${section.kind}`,
      label: t(SECTION_TITLES[section.kind]),
    });
    const keyOf = (path: string) => `${section.kind}:${path}`;
    const pathOf = new Map(section.entries.map((entry) => [keyOf(entry.path), entry.path]));
    section.entries.forEach((entry, position) => {
      const key = keyOf(entry.path);
      const favorite = isFavorite(props.folders, entry.path);
      const color = folderColor(props.folders, entry.path);
      const detail = entry.current ? t("folders.current") : entry.detail;
      const colorLabel = t("folders.colorNamed", { folder: entry.label });
      const reorderable = section.kind === "favorites" && section.entries.length > 1;
      items.push({
        type: "action",
        key,
        label: entry.label,
        ...(detail === "" ? {} : { detail }),
        title: reorderable ? `${entry.path}\n${moveHint}` : entry.path,
        current: entry.current,
        icon: entry.current ? (
          <CheckIcon size={14} />
        ) : (
          <span
            className={color === null ? "folder-menu-icon" : "folder-menu-icon tinted"}
            style={color === null ? undefined : { color: folderColorCss(color) }}
          >
            <FolderIcon size={14} />
          </span>
        ),
        secondary: [
          {
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
          {
            label: colorLabel,
            icon: (
              <span
                className="folder-color-dot"
                style={color === null ? undefined : { background: folderColorCss(color) }}
                aria-hidden="true"
              />
            ),
            className: color === null ? "folder-color" : "folder-color has-color",
            keepOpen: true,
            run: () => {
              setColoring((current) => (current === key ? null : key));
            },
          },
        ],
        // A running project stays listed while its sessions run; the x takes it off the saved
        // lists (favorites, recent), so it is offered wherever such an entry exists.
        ...(section.kind === "running" &&
        !frozen.prefs.favorites.some((path) => folderKey(path) === key) &&
        !frozen.prefs.recent.some((path) => folderKey(path) === key)
          ? {}
          : {
              remove: {
                label: t("folders.forgetNamed", { folder: entry.label }),
                run: () => {
                  setForgotten((current) => new Set(current).add(folderKey(entry.path)));
                  if (coloring === key) setColoring(null);
                  props.onForget(entry.path);
                },
              },
            }),
        ...(reorderable
          ? {
              reorder: {
                onStep: (step: 1 | -1) => {
                  const neighbour = section.entries[position + step];
                  if (neighbour !== undefined) move(entry.path, neighbour.path, step > 0);
                },
                onDrop: (targetKey: string, after: boolean) => {
                  const target = pathOf.get(targetKey);
                  if (target !== undefined) move(entry.path, target, after);
                },
              },
            }
          : {}),
        run: () => {
          props.onSelect(entry.path);
        },
      });
      if (coloring === key) {
        items.push({
          type: "swatches",
          key: `colors:${key}`,
          label: colorLabel,
          owner: key,
          options: [
            { key: NO_COLOR, label: t("folders.colorNone"), color: null },
            ...MARKER_COLORS.map((option) => ({
              key: option.id,
              label: t(MARKER_COLOR_NAMES[option.id]),
              color: folderColorCss(option.id),
            })),
          ],
          selected: color ?? NO_COLOR,
          onPick: (picked) => {
            const next = MARKER_COLORS.find((option) => option.id === picked)?.id ?? null;
            props.onColor(entry.path, next);
            setColoring(null);
          },
          onDismiss: () => {
            setColoring(null);
          },
        });
      }
    });
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
