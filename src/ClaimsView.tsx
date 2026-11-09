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
import {
  teammateOverlapText,
  teammateNotice,
  teammateOverlapKey,
  type TeammateOverlap,
} from "./team";

/** Changed places listed before "show more". */
const CLAIM_PAGE = 50;
/** Overlaps listed before "show more". */
const OVERLAP_PAGE = 20;

/**
 * Top of the Changes tab: a warning block when the active session collides with another one
 * (which session, which file, which lines, a jump there), a note when its folder is shared, and
 * the files it changed with their line ranges. Nothing at all while there is nothing to say.
 */
export function ClaimsSection(props: {
  claims: SessionClaims | null;
  overlaps: readonly Overlap[];
  /** Collisions with teammates' sessions in a shared project, worst first. */
  teammates: readonly TeammateOverlap[];
  nameOf: (id: number) => string;
  onShowSession: (id: number) => void;
}) {
  const { claims, overlaps, teammates, nameOf } = props;
  const t = useT();
  const [open, setOpen] = useState(false);
  const [listed, setListed] = useState(CLAIM_PAGE);
  const [listedOverlaps, setListedOverlaps] = useState(OVERLAP_PAGE);
  const [listedTeammates, setListedTeammates] = useState(OVERLAP_PAGE);
  if (claims === null) return null;
  if (claims.state.kind === "failed") {
    return (
      <p className="changes-hint selectable">
        <Revealed text={t("claims.failed", { error: claims.state.message })} />
      </p>
    );
  }
  if (claims.state.kind !== "ready") return null;
  const files = overlaps.filter((o) => isFileLevel(o.level));
  const areas = overlaps.filter((o) => !isFileLevel(o.level));
  const ordered = [...files, ...areas];
  // Past the backend's limit some overlaps are not in the snapshot at all.
  const unlisted = Math.max(0, claims.overlapCount - overlaps.length);
  return (
    <div className="claims">
      {teammates.length > 0 && (
        <section className="claims-warning" aria-labelledby="claims-team-title">
          <p className="claims-warning-title" id="claims-team-title">
            <OverlapIcon size={14} />
            <span>{t("claims.teammateTitle")}</span>
          </p>
          <ul className="claims-overlaps">
            {teammates.slice(0, listedTeammates).map((overlap) => (
              <li key={`${teammateOverlapKey(overlap)}\u0000${overlap.otherSession}`}>
                <span className="selectable">{teammateOverlapText(overlap, t)}</span>
              </li>
            ))}
          </ul>
          {teammates.length > listedTeammates && (
            <button
              type="button"
              className="link-button change-more files"
              onClick={() => {
                setListedTeammates((value) => value + OVERLAP_PAGE);
              }}
            >
              {t("claims.moreOverlaps", { count: teammates.length - listedTeammates })}
            </button>
          )}
          <p className="claims-help">{t("claims.warningHelp")}</p>
        </section>
      )}
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
            {ordered.slice(0, listedOverlaps).map((overlap) => (
              <li
                key={`${String(overlap.otherId)}:${overlap.level}:${overlap.path ?? overlap.area ?? ""}`}
              >
                <span className="selectable">
                  {overlapText(overlap, nameOf(overlap.otherId), t)}
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
          {ordered.length > listedOverlaps && (
            <button
              type="button"
              className="link-button change-more files"
              onClick={() => {
                setListedOverlaps((value) => value + OVERLAP_PAGE);
              }}
            >
              {t("claims.moreOverlaps", { count: ordered.length - listedOverlaps })}
            </button>
          )}
          {claims.overlapsTruncated && unlisted > 0 && (
            <p className="claims-help">{t("claims.unlisted", { count: unlisted })}</p>
          )}
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

/** The one-time notice of a new collision with a teammate, with a jump to our session. */
export function TeammateNoticeView(props: {
  overlap: TeammateOverlap;
  sessionName: string;
  onShow: () => void;
  onDismiss: () => void;
}) {
  const t = useT();
  const text = teammateNotice(props.overlap, props.sessionName, t);
  return (
    <div className="link-notice claim-notice" role="status">
      <span className="claim-notice-title">
        <OverlapIcon size={14} />
        <span>{text.title}</span>
      </span>
      <span className="link-url selectable">{text.body}</span>
      <div className="link-actions">
        <button type="button" className="secondary-button" onClick={props.onShow}>
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
