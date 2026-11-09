import { useState } from "react";

import {
  claimLines,
  formatLines,
  isFileLevel,
  overlapNotice,
  overlapText,
  type Overlap,
  type SessionClaims,
} from "./claims";
import { useT } from "./i18n";
import { ChevronIcon, CloseIcon, OverlapIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";

/** Changed places listed before "show more". */
const CLAIM_PAGE = 50;

/**
 * Top of the Changes tab: a warning block when the active session collides with another one
 * (which session, which file, which lines, a jump there), a note when its folder is shared, and
 * the files it changed with their line ranges. Nothing at all while there is nothing to say.
 */
export function ClaimsSection(props: {
  claims: SessionClaims | null;
  overlaps: readonly Overlap[];
  nameOf: (id: number) => string;
  onShowSession: (id: number) => void;
}) {
  const { claims, overlaps, nameOf } = props;
  const t = useT();
  const [open, setOpen] = useState(false);
  const [listed, setListed] = useState(CLAIM_PAGE);
  if (claims === null) return null;
  if (claims.state.kind === "failed") {
    return (
      <p className="changes-hint selectable">
        {t("claims.failed", { error: claims.state.message })}
      </p>
    );
  }
  if (claims.state.kind !== "ready") return null;
  const files = overlaps.filter((o) => isFileLevel(o.level));
  const areas = overlaps.filter((o) => !isFileLevel(o.level));
  return (
    <div className="claims">
      {overlaps.length > 0 && (
        <section
          className={files.length > 0 ? "claims-warning" : "claims-warning area"}
          aria-labelledby="claims-warning-title"
        >
          <p className="claims-warning-title" id="claims-warning-title">
            <OverlapIcon size={14} />
            <span>{t("claims.warningTitle")}</span>
          </p>
          <ul className="claims-overlaps">
            {[...files, ...areas].map((overlap) => (
              <li
                key={`${String(overlap.otherId)}:${overlap.level}:${overlap.path ?? overlap.area ?? ""}`}
              >
                <span className="selectable">
                  <Revealed text={overlapText(overlap, nameOf(overlap.otherId), t)} />
                </span>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    props.onShowSession(overlap.otherId);
                  }}
                >
                  {t("claims.showSession")}
                </button>
              </li>
            ))}
          </ul>
          {files.length > 0 && <p className="claims-help">{t("claims.warningHelp")}</p>}
        </section>
      )}
      {claims.sharedWith.length > 0 && (
        <p className="changes-hint">
          {t("claims.sharedFolder", { sessions: claims.sharedWith.map(nameOf).join(", ") })}
        </p>
      )}
      {claims.claims.length > 0 && (
        <div className={open ? "claims-lines open" : "claims-lines"}>
          <button
            type="button"
            className="claims-toggle"
            aria-expanded={open}
            onClick={() => {
              setOpen((value) => !value);
            }}
          >
            <ChevronIcon size={12} />
            <span>{t("claims.changedLines", { count: claims.claims.length })}</span>
          </button>
          {open && (
            <ul className="claims-files">
              {claims.claims.slice(0, listed).map((claim) => (
                <li key={claim.path} title={revealInvisible(claim.path)}>
                  <span className="claims-path">
                    <Revealed text={claim.path} />
                  </span>
                  <span className="claims-range">{claimLines(claim, t)}</span>
                </li>
              ))}
            </ul>
          )}
          {open && claims.claims.length > listed && (
            <button
              type="button"
              className="link-button change-more files"
              onClick={() => {
                setListed((value) => value + CLAIM_PAGE);
              }}
            >
              {t("changes.moreFiles", { count: claims.claims.length - listed })}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** The one-time notice of a new collision, with a jump to the other session. */
export function ClaimNoticeView(props: {
  overlap: Overlap;
  activeId: number | null;
  nameOf: (id: number) => string;
  onShow: (id: number) => void;
  onDismiss: () => void;
}) {
  const { overlap } = props;
  const t = useT();
  const text = overlapNotice(overlap, props.nameOf, t);
  const target = props.activeId === overlap.sessionId ? overlap.otherId : overlap.sessionId;
  return (
    <div className="link-notice claim-notice" role="status">
      <span className="claim-notice-title">
        <OverlapIcon size={14} />
        <span>{text.title}</span>
      </span>
      <span className="link-url selectable">
        {revealInvisible(overlap.path ?? "")}
        {overlap.level === "overlappingLines" && `, ${formatLines(overlap.lines, t)}`}
      </span>
      <div className="link-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            props.onShow(target);
          }}
        >
          {t("claims.showSession")}
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("error.dismiss")}
          title={t("error.dismiss")}
          onClick={props.onDismiss}
        >
          <CloseIcon />
        </button>
      </div>
    </div>
  );
}
