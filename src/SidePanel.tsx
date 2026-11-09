import { useRef } from "react";

import { useT } from "./i18n";
import { CloseIcon, DetachIcon } from "./icons";
import type { SideTab } from "./layout";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";

interface Props {
  tab: SideTab;
  onTab: (tab: SideTab) => void;
  /** `overlay`: over the window with a scrim (narrow window, see placeNotes). */
  mode: "docked" | "overlay";
  width: number;
  /** Notes live in their own window right now: no detach button. */
  detached: boolean;
  onDetach: () => void;
  onClose: () => void;
  /** Escape that nothing inside handled: back to the terminal. */
  onEscape: () => void;
  notes: React.ReactNode;
  changes: React.ReactNode;
}

const TABS: readonly SideTab[] = ["notes", "changes"];

/**
 * The right panel: one frame with the tabs "Notes | Changes", one header (detach, close) and the
 * content of the chosen tab. Docked next to the terminal, or in a narrow window as an overlay
 * with a scrim: a click outside or Escape closes it.
 */
export function SidePanel(props: Props) {
  const { tab, mode } = props;
  const t = useT();
  const tabs = useRef(new Map<SideTab, HTMLButtonElement>());
  const label = (id: SideTab) => (id === "notes" ? t("notes.title") : t("changes.title"));
  const keys = (id: SideTab) =>
    formatShortcut(id === "notes" ? SHORTCUT_LABELS.quickNote : SHORTCUT_LABELS.toggleChanges, t);

  const panel = (
    <aside
      className={mode === "overlay" ? "side-panel floating" : "side-panel"}
      style={{ width: props.width }}
      aria-label={t("side.label")}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || event.defaultPrevented) return;
        event.preventDefault();
        if (mode === "overlay") props.onClose();
        else props.onEscape();
      }}
    >
      <div className="side-header">
        <div className="side-tabs" role="tablist" aria-label={t("side.label")}>
          {TABS.map((id) => (
            <button
              key={id}
              ref={(element) => {
                if (element) tabs.current.set(id, element);
                else tabs.current.delete(id);
              }}
              type="button"
              role="tab"
              id={`side-tab-${id}`}
              className={id === tab ? "side-tab active" : "side-tab"}
              aria-selected={id === tab}
              aria-controls={`side-body-${id}`}
              tabIndex={id === tab ? 0 : -1}
              title={`${label(id)} (${keys(id)})`}
              onClick={() => {
                props.onTab(id);
              }}
              onKeyDown={(event) => {
                if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                event.preventDefault();
                const next = TABS[(TABS.indexOf(id) + 1) % TABS.length] ?? "notes";
                props.onTab(next);
                tabs.current.get(next)?.focus();
              }}
            >
              {label(id)}
            </button>
          ))}
        </div>
        {tab === "notes" && !props.detached && (
          <button
            type="button"
            className="icon-button"
            aria-label={t("notes.detach")}
            title={t("notes.detach")}
            onClick={props.onDetach}
          >
            <DetachIcon />
          </button>
        )}
        <button
          type="button"
          className="icon-button"
          aria-label={t("side.close")}
          title={t("side.close")}
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <div
        className="side-body"
        id="side-body-notes"
        role="tabpanel"
        aria-labelledby="side-tab-notes"
        hidden={tab !== "notes"}
      >
        {props.notes}
      </div>
      <div
        className="side-body"
        id="side-body-changes"
        role="tabpanel"
        aria-labelledby="side-tab-changes"
        hidden={tab !== "changes"}
      >
        {tab === "changes" && props.changes}
      </div>
    </aside>
  );

  if (mode === "docked") return panel;
  return (
    <>
      <div className="side-scrim" aria-hidden="true" onClick={props.onClose} />
      {panel}
    </>
  );
}
