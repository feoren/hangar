import { useT } from "./i18n";
import { SidePanelIcon } from "./icons";

/**
 * The bar above the work area, as tall as the sidebar's header so the line below both runs through
 * at one height: the active session's name on the left and, on the right above it, the switch for
 * the right panel (which has its own tabs).
 */
export function MainBar(props: {
  /** The active session's name, `null` without one (the home screen). */
  title: string | null;
  /** Whether the right panel is on screen in this window. */
  sideOpen: boolean;
  /** Opens the right panel on its last tab (or brings the detached panel up); open: closes it. */
  onToggleSide: () => void;
}) {
  const t = useT();
  const label = t("side.label");
  return (
    <div className="main-bar">
      <span className="main-bar-title" title={props.title ?? undefined}>
        {props.title}
      </span>
      <button
        type="button"
        className="icon-button main-bar-toggle"
        aria-label={label}
        aria-pressed={props.sideOpen}
        title={label}
        onClick={props.onToggleSide}
      >
        <SidePanelIcon />
      </button>
    </div>
  );
}
