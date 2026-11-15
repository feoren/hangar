import type { Profile } from "./api";
import { Badge } from "./Badge";
import { Dialog } from "./Dialog";
import type { BadgeLook } from "./programPrefs";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";

interface Props {
  /** Title of the session that is handed over. */
  title: string;
  /** Installed agents that can take over (handover.ts `handoverTargets`). */
  targets: readonly Profile[];
  badgeFor: (profileId: string) => BadgeLook;
  /** The old agent runs and is idle: it can be asked for a summary first. */
  canSummarize: boolean;
  onSummarize: () => void;
  onPick: (profile: Profile) => void;
  onClose: () => void;
}

/**
 * "Hand over to": one button per agent that can take over. Hangar starts it in the same folder
 * and pastes a handover text without Enter; optionally the old agent is asked for a summary first.
 */
export function HandoverDialog({
  title,
  targets,
  badgeFor,
  canSummarize,
  onSummarize,
  onPick,
  onClose,
}: Props) {
  const t = useT();
  return (
    <Dialog labelledBy="handover-title" className="handover-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="handover-title">
          {t("handover.title")}
          <span className="marker-dialog-session">{title}</span>
        </h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <p className="muted handover-help">{t("handover.help")}</p>
      {targets.length === 0 ? (
        <p className="muted">{t("handover.noTargets")}</p>
      ) : (
        <ul className="handover-list">
          {targets.map((profile, index) => (
            <li key={profile.id}>
              <button
                type="button"
                className="handover-target"
                data-autofocus={index === 0 ? true : undefined}
                onClick={() => {
                  onPick(profile);
                }}
              >
                <Badge look={badgeFor(profile.id)} />
                <span>{profile.name}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {canSummarize && (
        <div className="handover-summary">
          <button type="button" className="secondary-button" onClick={onSummarize}>
            {t("handover.summarize")}
          </button>
          <span className="muted">{t("handover.summarizeHelp")}</span>
        </div>
      )}
    </Dialog>
  );
}
