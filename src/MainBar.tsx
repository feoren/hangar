import { useT } from "./i18n";
import { FilesIcon, NotePencilIcon, SourceControlIcon } from "./icons";
import { SIDE_TAB_LABELS, type SideTab } from "./layout";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";

/**
 * The bar above the work area, as tall as the sidebar's header so the line below both runs through
 * at one height: the active session's name on the left, the right panel's tabs on the right, above
 * the panel they open.
 */
export function MainBar(props: {
  /** The active session's name, `null` without one (the home screen). */
  title: string | null;
  /** The right panel's tab on screen in this window, `null` when it is closed or detached. */
  sideTab: SideTab | null;
  /** Opens the right panel on this tab (or brings the detached panel to it); shown again: closes. */
  onToggleSideTab: (tab: SideTab) => void;
}) {
  const t = useT();
  return (
    <div className="main-bar">
      <span className="main-bar-title" title={props.title ?? undefined}>
        {props.title}
      </span>
      <span className="main-bar-tabs">
        {SIDE_TABS.map(({ tab, Icon }) => {
          const label = t(SIDE_TAB_LABELS[tab]);
          return (
            <button
              key={tab}
              type="button"
              className="icon-button main-bar-tab"
              aria-label={label}
              aria-pressed={props.sideTab === tab}
              title={
                tab === "notes"
                  ? `${label}\n${t("notes.quickNoteHint", { keys: formatShortcut(SHORTCUT_LABELS.quickNote, t) })}`
                  : label
              }
              onClick={() => {
                props.onToggleSideTab(tab);
              }}
            >
              <Icon />
            </button>
          );
        })}
      </span>
    </div>
  );
}

/** The right panel's tabs in its own order, each with its icon. */
const SIDE_TABS = [
  { tab: "notes", Icon: NotePencilIcon },
  { tab: "changes", Icon: SourceControlIcon },
  { tab: "files", Icon: FilesIcon },
] as const satisfies readonly {
  tab: SideTab;
  Icon: (props: { size?: number }) => React.ReactNode;
}[];
