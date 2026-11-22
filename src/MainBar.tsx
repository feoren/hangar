import { api } from "./api";
import { useT } from "./i18n";
import { CoffeeIcon, SidePanelIcon } from "./icons";

/**
 * The bar above the work area, as tall as the sidebar's header so the line below both runs through
 * at one height: the active session's name on the left and, on the right above it, the support
 * link and, outermost, the switch for the right panel (which has its own tabs).
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
  const coffeeLabel = t("sidebar.coffee");
  return (
    <div className="main-bar">
      <span className="main-bar-title" title={props.title ?? undefined}>
        {props.title}
      </span>
      <button
        type="button"
        className="main-bar-coffee"
        title={coffeeLabel}
        onClick={() => {
          api.openWebUrl(SUPPORT_URL).catch((error: unknown) => {
            console.warn("could not open the support page", error);
          });
        }}
      >
        <CoffeeIcon size={14} />
        <span className="main-bar-coffee-text">{coffeeLabel}</span>
      </button>
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

/** The project's Buy Me a Coffee page, opened in the default browser. */
export const SUPPORT_URL = "https://buymeacoffee.com/feoren";
