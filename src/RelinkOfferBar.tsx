import { useT } from "./i18n";

/** What the bar says about one remembered link group (linkOffers.ts). */
export interface RelinkOfferView {
  /** The members whose session runs now, as the sidebar names them. */
  names: readonly string[];
  /** Member names of remembered members whose session does not run. */
  missing: readonly string[];
}

/**
 * Below the session list after a restart, one remembered link group at a time (ADR 0014): who
 * formed it, who is missing, "Link" and "Discard". Same place and shape as the selection bar:
 * it concerns the rows above, stays until the person decides, and never covers the terminal.
 */
export function RelinkOfferBar(props: {
  offer: RelinkOfferView;
  onDecide: (apply: boolean) => void;
}) {
  const t = useT();
  return (
    <div className="link-selection link-offer" role="group" aria-label={t("links.offerTitle")}>
      <div className="link-selection-text">
        <span className="link-selection-count">{t("links.offerTitle")}</span>
        <span className="link-selection-hint">{props.offer.names.join(", ")}</span>
        {props.offer.missing.length > 0 && (
          <span className="link-selection-hint">
            {t("links.offerMissing", { names: props.offer.missing.join(", ") })}
          </span>
        )}
      </div>
      <button
        type="button"
        className="primary-button link-selection-link"
        onClick={() => {
          props.onDecide(true);
        }}
      >
        {t("links.offerLink")}
      </button>
      <button
        type="button"
        className="link-button"
        onClick={() => {
          props.onDecide(false);
        }}
      >
        {t("links.offerDiscard")}
      </button>
    </div>
  );
}
