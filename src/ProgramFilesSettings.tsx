// Settings section "Terminals & Agents", lower part: HANGAR.md for every agent, and under
// "Advanced" the upstream check of a fresh worktree session and the user's own profiles.toml.

import { useState } from "react";

import { errorText, type Profile } from "./api";
import { useT } from "./i18n";
import { profileErrorText } from "./profileFile";
import type { Settings } from "./settings";
import { ActionRow, Advanced, SettingRow, SettingsGroup, SwitchRow } from "./SettingsParts";
import { instructionsReached, instructionsUnavailable } from "./startOptions";

type Patch = (patch: Partial<Settings>) => void;

/**
 * The HANGAR.md switch. While it is on, the line below names the programs it reaches (and the ones
 * it cannot reach here) and opens the file.
 */
export function AgentInstructionsGroup({
  settings,
  profiles,
  onChange,
  onOpen,
}: {
  settings: Settings;
  profiles: Profile[];
  onChange: Patch;
  /** Opens HANGAR.md in the default editor (creates it if needed); rejects with an error. */
  onOpen: () => Promise<void>;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const reached = instructionsReached(profiles);
  const unavailable = instructionsUnavailable(profiles);
  const reach = [
    reached.length > 0 ? t("settings.hangarMdReach", { names: reached.join(", ") }) : "",
    unavailable.length > 0
      ? t("settings.hangarMdUnavailable", { names: unavailable.join(", ") })
      : "",
  ]
    .filter(Boolean)
    .join(" ");

  const open = () => {
    setBusy(true);
    setProblem(null);
    onOpen()
      .catch((error: unknown) => {
        setProblem(errorText(error));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <SettingsGroup title={t("settings.agentInstructions")}>
      <SwitchRow
        label={t("settings.hangarMd")}
        help={t("settings.hangarMdHelp")}
        checked={settings.hangarMd}
        onChange={(hangarMd) => {
          onChange({ hangarMd });
        }}
      >
        <ActionRow
          status={problem ?? reach}
          error={problem !== null}
          action={t("settings.openHangarMd")}
          disabled={busy}
          onAction={open}
        />
      </SwitchRow>
    </SettingsGroup>
  );
}

/** Rarely changed: the upstream check in a fresh worktree, and the user's own profiles.toml. */
export function ProgramsAdvanced({
  settings,
  onChange,
  onOpenProfileFile,
  onReloadProfiles,
}: {
  settings: Settings;
  onChange: Patch;
  /** Opens profiles.toml in the default editor (creates it if needed); rejects with an error. */
  onOpenProfileFile: () => Promise<void>;
  /** Rereads profiles.toml; rejects with the backend's error (profileFile.ts). */
  onReloadProfiles: () => Promise<void>;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);

  const run = (action: () => Promise<void>, done: string | null) => {
    setBusy(true);
    setStatus(null);
    action()
      .then(() => {
        if (done !== null) setStatus({ ok: true, text: done });
      })
      .catch((error: unknown) => {
        setStatus({ ok: false, text: profileErrorText(error, t) });
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <Advanced>
      <SettingsGroup>
        <SwitchRow
          label={t("settings.worktreeUpstreamCheck")}
          help={t("settings.worktreeUpstreamCheckHelp")}
          checked={settings.worktreeUpstreamCheck}
          onChange={(worktreeUpstreamCheck) => {
            onChange({ worktreeUpstreamCheck });
          }}
        />
        <SettingRow
          label={t("settings.profileFile")}
          help={status?.text ?? t("settings.profileFileHelp")}
          helpIsStatus
          helpIsError={status !== null && !status.ok}
          control={({ labelId }) => (
            <>
              <button
                type="button"
                className="secondary-button"
                aria-describedby={labelId}
                disabled={busy}
                onClick={() => {
                  run(onOpenProfileFile, null);
                }}
              >
                {t("settings.openProfileFile")}
              </button>
              <button
                type="button"
                className="secondary-button"
                aria-describedby={labelId}
                disabled={busy}
                onClick={() => {
                  run(onReloadProfiles, t("settings.profilesReloaded"));
                }}
              >
                {t("settings.reloadProfiles")}
              </button>
            </>
          )}
        />
      </SettingsGroup>
    </Advanced>
  );
}
