import { shortPath } from "./format";
import { folderName, isFavorite, recentOnly, samePath, type FolderPrefs } from "./folders";
import { useT } from "./i18n";
import { ClockIcon, FolderIcon, StarIcon } from "./icons";
import { PopupMenu, type PopupItem } from "./PopupMenu";

interface Props {
  /** Current working folder for new sessions. */
  cwd: string | null;
  folders: FolderPrefs;
  position: { x: number; y: number };
  /** Top edge of the opening button, so the menu can open above it (PopupMenu). */
  flipAbove?: number;
  anchor?: React.RefObject<HTMLElement | null>;
  onSelect: (path: string) => void;
  /** Opens the system's folder dialog. */
  onChoose: () => void;
  onToggleFavorite: (path: string) => void;
  onClose: (reason: "action" | "dismiss") => void;
}

/**
 * "Starts in" menu: favorites, recently used folders, the folder dialog and adding or removing
 * the current folder as a favorite. Used by the sidebar and the empty state.
 */
export function FolderMenu(props: Props) {
  const t = useT();
  const { cwd, folders } = props;

  const entry = (path: string, kind: "favorite" | "recent"): PopupItem => {
    const current = cwd !== null && samePath(path, cwd);
    const parent = path.slice(0, Math.max(0, path.length - folderName(path).length));
    const detail = current ? t("folders.current") : parent === "" ? undefined : shortPath(parent);
    return {
      type: "action",
      key: `${kind}:${path}`,
      label: folderName(path),
      ...(detail === undefined ? {} : { detail }),
      title: path,
      icon: kind === "favorite" ? <StarIcon filled /> : <ClockIcon />,
      run: () => {
        props.onSelect(path);
      },
    };
  };

  const favorites = folders.favorites.map((path) => entry(path, "favorite"));
  const recent = recentOnly(folders).map((path) => entry(path, "recent"));
  const favorite = cwd !== null && isFavorite(folders, cwd);
  const items: PopupItem[] = [
    ...favorites,
    ...(favorites.length > 0 && recent.length > 0
      ? [{ type: "separator", key: "sep-recent" } as const]
      : []),
    ...recent,
    ...(favorites.length + recent.length > 0 ? [{ type: "separator", key: "sep" } as const] : []),
    {
      type: "action",
      key: "choose",
      label: t("menu.pickCwd"),
      icon: <FolderIcon />,
      run: props.onChoose,
    },
    {
      type: "action",
      key: "favorite",
      label: favorite ? t("folders.removeFavorite") : t("folders.addFavorite"),
      icon: <StarIcon filled={!favorite} />,
      disabled: cwd === null,
      run: () => {
        if (cwd !== null) props.onToggleFavorite(cwd);
      },
    },
  ];

  return (
    <PopupMenu
      items={items}
      label={t("folders.menu")}
      position={props.position}
      {...(props.flipAbove === undefined ? {} : { flipAbove: props.flipAbove })}
      {...(props.anchor === undefined ? {} : { anchor: props.anchor })}
      onClose={props.onClose}
    />
  );
}
