import { useState, type ReactNode } from "react";

import { api } from "./api";
import { useT } from "./i18n";
import { PRICING_URL } from "./teamPlan";

/** Opens the pricing page in the browser; resolves to whether that worked. */
export function openPricing(): Promise<boolean> {
  return api.openWebUrl(PRICING_URL).then(
    () => true,
    () => false,
  );
}

/**
 * "See plans" as a quiet link: opens the pricing page, also for a self-hosted server, since
 * licenses for it are bought there too. Says so when the browser could not be opened.
 */
export function SeePlansButton() {
  const t = useT();
  const [failed, setFailed] = useState(false);
  return (
    <>
      <button
        type="button"
        className="link-button team-plan-link"
        onClick={() => {
          setFailed(false);
          void openPricing().then((ok) => {
            setFailed(!ok);
          });
        }}
      >
        {t("teamPlan.seePlans")}
      </button>
      {failed && (
        <span className="team-help error-text" role="alert">
          {t("account.error.open")}
        </span>
      )}
    </>
  );
}

/**
 * Why team functions are missing, in their place (teamPlan.ts): teams are part of the Team plan,
 * Free is for one person. `onSignIn`: signed out, the person may still belong to a paid team, so a
 * sign-in follows. `extra`: one more quiet action, e.g. reloading the projects after a purchase.
 * No greyed-out controls: only what works is offered.
 */
export function TeamPlanNote(props: { onSignIn?: (() => void) | undefined; extra?: ReactNode }) {
  const t = useT();
  return (
    <div className="team-plan-note">
      <p className="team-plan-text">
        {t("teamPlan.text")}
        {props.onSignIn !== undefined && <> {t("teamPlan.signedOut")}</>}
      </p>
      <div className="team-plan-actions">
        <SeePlansButton />
        {props.onSignIn !== undefined && (
          <button type="button" className="link-button team-plan-link" onClick={props.onSignIn}>
            {t("account.signIn")}
          </button>
        )}
        {props.extra}
      </div>
    </div>
  );
}
