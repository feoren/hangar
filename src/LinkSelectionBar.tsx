import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import type { SelectionProblem } from "./links";

/** What "Link" would do with the selection: something (`ready`), or why not. */
export type SelectionState = "ready" | SelectionProblem;

/**
 * Below the session list while rows are selected with Ctrl or Cmd click (ADR 0014): the count,
 * "Link", and the way out. Where "Link" cannot work, the button stays visible but off and one
 * short line says why; after a Ctrl or Cmd click on a session that cannot be linked, the line
 * says that instead (`refused`), also without a selection.
 */
export function LinkSelectionBar(props: {
  count: number;
  state: SelectionState;
  /** A Ctrl or Cmd click just went to a session that cannot be linked. */
  refused: boolean;
  /** The select click's name ("Ctrl+click", shortcuts.ts `selectClickLabel`). */
  clickLabel: string;
  onLink: () => void;
  onClear: () => void;
}) {
  const t = useT();
  const hint = props.refused
    ? t("linkError.notLinkable")
    : props.count === 0 || props.state === "ready"
      ? ""
      : props.state === "more"
        ? t("links.selectMore", { click: props.clickLabel })
        : t("linkError.tooMany");
  return (
    <div className="link-selection" role="group" aria-label={t("links.title")}>
      <div className="link-selection-text">
        {props.count > 0 && (
          <span className="link-selection-count">
            {t("links.selected", { count: props.count })}
          </span>
        )}
        <span className="link-selection-hint" role="status">
          {hint}
        </span>
      </div>
      {props.count > 0 && (
        <button
          type="button"
          className="primary-button link-selection-link"
          disabled={props.state !== "ready"}
          onClick={props.onLink}
        >
          {t("links.link")}
        </button>
      )}
      <button
        type="button"
        className="icon-button"
        aria-label={t("links.clearSelection")}
        title={t("links.clearSelection")}
        onClick={props.onClear}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
