import { useRef, useState, type ReactNode } from "react";

import { errorText } from "./api";
import { budgetFromInput, EARLY_PERCENTS, parseEarlyPercent } from "./costs";
import { isLanguage, LANGUAGE_NAMES, LANGUAGES, useT } from "./i18n";
import { powerKnown } from "./power";
import { RangeField } from "./RangeField";
import { clampScrollbackLines, SCROLLBACK_LINES, type Settings } from "./settings";
import { ActionRow, InlineConfirm, SettingRow, SettingsGroup, SwitchRow } from "./SettingsParts";
import { usePowerState } from "./usePower";

interface Props {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
  /** Deletes all saved scrollback; rejects with a readable error. */
  onClearScrollbacks: () => Promise<void>;
}

/**
 * Settings section "General": the language, what happens when the window closes, and what the
 * sessions cost in resources and money.
 */
export function GeneralSettings({ settings, onChange, onClearScrollbacks }: Props) {
  const t = useT();
  const power = usePowerState();
  return (
    <>
      <SettingsGroup>
        <SettingRow
          label={t("settings.language")}
          help={t("settings.languageHelp")}
          control={({ labelId, helpId }) => (
            // A list instead of buttons: there are many languages.
            <select
              id="settings-language"
              className="select"
              value={settings.language}
              aria-labelledby={labelId}
              aria-describedby={helpId}
              onChange={(event) => {
                const language = event.currentTarget.value;
                if (isLanguage(language)) onChange({ language });
              }}
            >
              {LANGUAGES.map((language) => (
                <option key={language} value={language} lang={language}>
                  {LANGUAGE_NAMES[language]}
                </option>
              ))}
            </select>
          )}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.groupOnClose")}>
        <SwitchRow
          label={t("settings.keepInTray")}
          help={t("settings.keepInTrayHelp")}
          checked={settings.keepInTray}
          onChange={(keepInTray) => {
            onChange({ keepInTray });
          }}
        />
        <SwitchRow
          label={t("settings.restoreSessions")}
          help={t("settings.restoreSessionsHelp")}
          checked={settings.restoreSessions}
          onChange={(restoreSessions) => {
            onChange({ restoreSessions });
          }}
        >
          <div className="setting">
            <div className="setting-line">
              <RangeField
                id="scrollback-lines"
                label={t("settings.scrollbackLines")}
                value={settings.scrollbackLines}
                min={SCROLLBACK_LINES.min}
                max={SCROLLBACK_LINES.max}
                step={SCROLLBACK_LINES.step}
                display={t("settings.lines", {
                  count: settings.scrollbackLines.toLocaleString(settings.language),
                })}
                onChange={(value) => {
                  onChange({ scrollbackLines: clampScrollbackLines(value) });
                }}
              />
            </div>
          </div>
          <ClearScrollbacks onClear={onClearScrollbacks} />
        </SwitchRow>
      </SettingsGroup>

      <SettingsGroup title={t("settings.groupPerformance")}>
        <SwitchRow
          label={t("settings.showResources")}
          help={t("settings.showResourcesHelp")}
          checked={settings.showResources}
          onChange={(showResources) => {
            onChange({ showResources });
          }}
        />
        {/* Only where the backend can tell battery from AC (only offer what works). */}
        {powerKnown(power) && (
          <SwitchRow
            label={t("settings.powerSaving")}
            help={t("settings.powerSavingHelp")}
            checked={settings.powerSaving}
            onChange={(powerSaving) => {
              onChange({ powerSaving });
            }}
          />
        )}
        <BudgetRow
          value={settings.dailyBudget}
          onChange={(dailyBudget) => {
            onChange({ dailyBudget });
          }}
        >
          {/* Only with a budget: without one there is nothing to warn early about. */}
          <EarlyWarningRow
            value={settings.budgetEarlyPercent}
            onChange={(budgetEarlyPercent) => {
              onChange({ budgetEarlyPercent });
            }}
          />
        </BudgetRow>
        <SwitchRow
          label={t("morning.setting")}
          help={t("morning.settingHelp")}
          checked={settings.morningReview}
          onChange={(morningReview) => {
            onChange({ morningReview });
          }}
        />
      </SettingsGroup>
    </>
  );
}

/** "Delete histories …" with the question in its place and the result next to the button. */
function ClearScrollbacks({ onClear }: { onClear: () => Promise<void> }) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const button = useRef<HTMLButtonElement>(null);

  // The button is back in the page only after the question closed: focus it on the next frame.
  const backToButton = () => {
    requestAnimationFrame(() => {
      button.current?.focus();
    });
  };

  if (confirming) {
    return (
      <div className="setting">
        <InlineConfirm
          text={t("settings.clearConfirm")}
          confirm={t("settings.clearYes")}
          onConfirm={() => {
            setConfirming(false);
            onClear()
              .then(() => {
                setStatus({ ok: true, text: t("settings.clearScrollbacksDone") });
              })
              .catch((error: unknown) => {
                setStatus({ ok: false, text: errorText(error) });
              })
              .finally(backToButton);
          }}
          onCancel={() => {
            setConfirming(false);
            backToButton();
          }}
        />
      </div>
    );
  }
  return (
    <ActionRow
      status={status ? status.text : t("settings.clearScrollbacksHelp")}
      error={status !== null && !status.ok}
      action={t("settings.clearScrollbacks")}
      buttonRef={button}
      onAction={() => {
        setStatus(null);
        setConfirming(true);
      }}
    />
  );
}

/**
 * Daily budget in dollars: taken when the field loses focus or Enter is pressed; an empty field
 * means no budget, an invalid entry goes back to the saved value. `children` (the early warning)
 * show only while a budget is set.
 */
function BudgetRow({
  value,
  onChange,
  children,
}: {
  value: number | null;
  onChange: (value: number | null) => void;
  children: ReactNode;
}) {
  const t = useT();
  const [text, setText] = useState(value === null ? "" : String(value));
  const commit = () => {
    const parsed = budgetFromInput(text);
    if (parsed === null && text.trim() !== "") {
      setText(value === null ? "" : String(value));
      return;
    }
    setText(parsed === null ? "" : String(parsed));
    if (parsed !== value) onChange(parsed);
  };
  return (
    <SettingRow
      label={t("costs.budget")}
      help={t("costs.budgetHelp")}
      expanded={value !== null}
      control={({ labelId, helpId }) => (
        <input
          id="daily-budget"
          type="text"
          inputMode="decimal"
          className="text-input budget-input"
          autoComplete="off"
          spellCheck={false}
          placeholder={t("costs.budgetNone")}
          value={text}
          aria-labelledby={labelId}
          aria-describedby={helpId}
          onChange={(event) => {
            setText(event.currentTarget.value);
          }}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
        />
      )}
    >
      {children}
    </SettingRow>
  );
}

/**
 * The early budget warning: off, or a share of the budget. A stored share that is not one of the
 * choices (the backend takes any from 1 to 99) is listed as well, so it never reads as off.
 */
function EarlyWarningRow({
  value,
  onChange,
}: {
  value: number | null;
  onChange: (value: number | null) => void;
}) {
  const t = useT();
  const choices: readonly number[] =
    value === null || EARLY_PERCENTS.some((choice) => choice === value)
      ? EARLY_PERCENTS
      : [...EARLY_PERCENTS, value].sort((a, b) => a - b);
  return (
    <SettingRow
      label={t("costs.earlyWarning")}
      help={t("costs.earlyHelp")}
      control={({ labelId, helpId }) => (
        <select
          id="budget-early"
          className="select budget-select"
          value={value === null ? "off" : String(value)}
          aria-labelledby={labelId}
          aria-describedby={helpId}
          onChange={(event) => {
            onChange(parseEarlyPercent(Number(event.currentTarget.value)));
          }}
        >
          <option value="off">{t("costs.earlyOff")}</option>
          {choices.map((choice) => (
            <option key={choice} value={String(choice)}>
              {t("settings.percent", { count: choice })}
            </option>
          ))}
        </select>
      )}
    />
  );
}
