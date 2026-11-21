import { GUARDED_PROGRAMS } from "./commandGuard";
import { CommandGuardFile, guardNeedsAttention, useCommandGuardInfo } from "./CommandGuardSettings";
import { GateLog, GateRuleList, useGateRules } from "./GateRulesSettings";
import { useT } from "./i18n";
import type { Settings } from "./settings";
import { Advanced, SettingsGroup, SwitchRow } from "./SettingsParts";

/**
 * Settings section "Security": the question before a risky paste, then the overlap warning with
 * what builds on it. The gate needs the overlaps it pauses for, and the command guard asks through
 * the gate, so each shows only while the one above it is on. The rules file and the log of
 * automatic answers are rarely needed and wait under "Advanced"; the remembered answers show as
 * soon as there is one, so it can be deleted.
 */
export function SecuritySettings({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  const info = useCommandGuardInfo(settings.commandGuardEnabled);
  const { rules, log, remove } = useGateRules();
  const gateOn = settings.claimsEnabled && settings.gateEnabled;
  const guardOn = gateOn && settings.commandGuardEnabled;
  const showFile = guardOn && info !== null;
  const showLog = gateOn && log.length > 0;

  return (
    <>
      <SettingsGroup>
        <SwitchRow
          label={t("settings.warnMultilinePaste")}
          help={t("settings.warnMultilinePasteHelp")}
          checked={settings.warnMultilinePaste}
          onChange={(warnMultilinePaste) => {
            onChange({ warnMultilinePaste });
          }}
        />
        <SwitchRow
          label={t("settings.claims")}
          help={t("settings.claimsHelp")}
          checked={settings.claimsEnabled}
          onChange={(claimsEnabled) => {
            onChange({ claimsEnabled });
          }}
        >
          <SwitchRow
            label={t("settings.gate")}
            help={t("settings.gateHelp")}
            checked={settings.gateEnabled}
            onChange={(gateEnabled) => {
              onChange({ gateEnabled });
            }}
          >
            <SwitchRow
              label={t("settings.commandGuard")}
              help={t("settings.commandGuardHelp", { programs: GUARDED_PROGRAMS })}
              checked={settings.commandGuardEnabled}
              onChange={(commandGuardEnabled) => {
                onChange({ commandGuardEnabled });
              }}
            />
          </SwitchRow>
        </SwitchRow>
      </SettingsGroup>

      {rules.length > 0 && (
        <SettingsGroup title={t("settings.gateRules")} description={t("settings.gateRulesHelp")}>
          <GateRuleList rules={rules} label={t("settings.gateRules")} onRemove={remove} />
        </SettingsGroup>
      )}

      {(showFile || showLog) && (
        // A rules file with problems opens this part, so the problem is not hidden.
        <Advanced defaultOpen={guardOn && guardNeedsAttention(info)}>
          {showFile && <CommandGuardFile info={info} />}
          {showLog && <GateLog log={log} language={settings.language} />}
        </Advanced>
      )}
    </>
  );
}
