import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { errorText, type Profile } from "./api";

import { Dialog } from "./Dialog";
import {
  isLanguage,
  LANGUAGE_NAMES,
  LANGUAGES,
  useT,
  type MessageKey,
  type Translate,
} from "./i18n";
import { CloseIcon } from "./icons";
import { clampFontSize, FONT_SIZE } from "./layout";
import { profileErrorText } from "./profileFile";
import type { BadgeLook } from "./programPrefs";
import type { ProgramVersions } from "./programs";
import { ProgramsSettings } from "./ProgramsSettings";
import {
  ATTENTION_PRESETS,
  clampQuietSeconds,
  clampScrollbackLines,
  normalizeColor,
  QUIET_SECONDS,
  SCROLLBACK_LINES,
  type Settings,
} from "./settings";
import { nextTabIndex, SETTINGS_SECTIONS, type SettingsSection } from "./settingsSections";
import { THEME_SETTINGS, type ThemeSetting } from "./theme";
import type { ProgramUpdateState } from "./useProgramUpdates";
import type { UpdateState } from "./useUpdater";
import { displayVersion } from "./version";
import { VersionText } from "./VersionText";

interface Props {
  settings: Settings;
  profiles: Profile[];
  versions: ProgramVersions;
  badgeFor: (profileId: string) => BadgeLook;
  fontSize: number;
  update: UpdateState;
  /** Version of Hangar, `null` until known. */
  version: string | null;
  programUpdates: ProgramUpdateState;
  /** Profile IDs with a running session (hint about locked files during an update). */
  runningPrograms: ReadonlySet<string>;
  /** Launch a program's update as a session (only with `canRun`). */
  onRunUpdate: (profileId: string) => void;
  onChange: (patch: Partial<Settings>) => void;
  /** Deletes all saved scrollback; rejects with a readable error. */
  onClearScrollbacks: () => Promise<void>;
  /** Opens profiles.toml in the default editor (creates it if needed); rejects with an error. */
  onOpenProfileFile: () => Promise<void>;
  /** Rereads profiles.toml; rejects with the backend's error (profileFile.ts). */
  onReloadProfiles: () => Promise<void>;
  onFontSize: (size: number) => void;
  onResetLayout: () => void;
  onCheckUpdates: () => void;
  onClose: () => void;
}

const COLOR_NAMES: Record<(typeof ATTENTION_PRESETS)[number]["id"], MessageKey> = {
  cream: "color.cream",
  amber: "color.amber",
  mint: "color.mint",
  sky: "color.sky",
};

const SECTION_NAMES: Record<SettingsSection, MessageKey> = {
  general: "settings.general",
  appearance: "settings.appearance",
  attention: "settings.attention",
  programs: "settings.programs",
  updates: "settings.updates",
};

const THEME_NAMES: Record<ThemeSetting, MessageKey> = {
  system: "settings.themeSystem",
  light: "settings.themeLight",
  dark: "settings.themeDark",
};

/** Same breakpoint as in styles.css: below it, the sections sit in a row above the content. */
const NARROW_QUERY = "(max-width: 620px)";

function subscribeNarrow(onChange: () => void): () => void {
  const query = window.matchMedia(NARROW_QUERY);
  query.addEventListener("change", onChange);
  return () => {
    query.removeEventListener("change", onChange);
  };
}

function isNarrow(): boolean {
  return window.matchMedia(NARROW_QUERY).matches;
}

/** Most recently chosen section while Hangar runs. After a restart it starts at General. */
let lastSection: SettingsSection = "general";

/**
 * Sections as vertical tabs on the left (WAI-ARIA Tabs, automatic activation), content on the
 * right. Every change takes effect immediately and is saved right away; there is no Save
 * button.
 */
export function SettingsDialog(props: Props) {
  const t = useT();
  const [section, setSection] = useState<SettingsSection>(lastSection);
  const tabs = useRef(new Map<SettingsSection, HTMLButtonElement>());
  const narrow = useSyncExternalStore(subscribeNarrow, isNarrow);

  useEffect(() => {
    lastSection = section;
  }, [section]);

  const select = (next: SettingsSection, focus: boolean) => {
    setSection(next);
    if (focus) tabs.current.get(next)?.focus();
  };

  const onTabKey = (event: React.KeyboardEvent) => {
    const index = SETTINGS_SECTIONS.indexOf(section);
    const next = nextTabIndex(event.key, index, SETTINGS_SECTIONS.length);
    const target = next === null ? undefined : SETTINGS_SECTIONS[next];
    if (target === undefined) return;
    event.preventDefault();
    select(target, true);
  };

  return (
    <Dialog labelledBy="settings-title" className="settings" onClose={props.onClose}>
      <div className="dialog-header">
        <h2 id="settings-title">{t("settings.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="settings-body">
        <div
          className="settings-tabs"
          role="tablist"
          aria-orientation={narrow ? "horizontal" : "vertical"}
          aria-label={t("settings.sections")}
          onKeyDown={onTabKey}
        >
          {SETTINGS_SECTIONS.map((id) => {
            const selected = id === section;
            return (
              <button
                key={id}
                ref={(element) => {
                  if (element) tabs.current.set(id, element);
                  else tabs.current.delete(id);
                }}
                type="button"
                role="tab"
                id={`settings-tab-${id}`}
                className="settings-tab"
                aria-selected={selected}
                aria-controls={selected ? `settings-panel-${id}` : undefined}
                tabIndex={selected ? 0 : -1}
                data-autofocus={selected ? true : undefined}
                onClick={() => {
                  select(id, false);
                }}
              >
                {t(SECTION_NAMES[id])}
              </button>
            );
          })}
        </div>

        <div
          className="settings-panel"
          role="tabpanel"
          id={`settings-panel-${section}`}
          aria-labelledby={`settings-tab-${section}`}
          tabIndex={0}
        >
          <h3 className="settings-panel-title">{t(SECTION_NAMES[section])}</h3>
          <SectionContent section={section} {...props} />
        </div>
      </div>

      <div className="dialog-actions">
        <button type="button" className="primary-button" onClick={props.onClose}>
          {t("common.close")}
        </button>
      </div>
    </Dialog>
  );
}

function SectionContent(props: Props & { section: SettingsSection }) {
  switch (props.section) {
    case "general":
      return <GeneralSection {...props} />;
    case "appearance":
      return <AppearanceSection {...props} />;
    case "attention":
      return <AttentionSection {...props} />;
    case "programs":
      return <ProgramsSection {...props} />;
    case "updates":
      return <UpdatesSection {...props} />;
  }
}

function GeneralSection(props: Props) {
  const { settings, onChange } = props;
  return (
    <>
      <LanguageField settings={settings} onChange={onChange} />
      <SessionsField {...props} />
    </>
  );
}

function LanguageField({ settings, onChange }: Pick<Props, "settings" | "onChange">) {
  const t = useT();
  return (
    <div className="field">
      <label htmlFor="settings-language">{t("settings.language")}</label>
      {/* A list instead of buttons: more languages are coming. */}
      <select
        id="settings-language"
        className="select"
        value={settings.language}
        aria-describedby="settings-language-help"
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
      <p id="settings-language-help" className="muted">
        {t("settings.languageHelp")}
      </p>
    </div>
  );
}

/** Remember sessions on exit, scrollback length, delete saved scrollback. */
function SessionsField({ settings, onChange, onClearScrollbacks }: Props) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const clearButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (confirming) cancelButton.current?.focus();
  }, [confirming]);

  const clear = () => {
    setConfirming(false);
    onClearScrollbacks()
      .then(() => {
        setStatus({ ok: true, text: t("settings.clearScrollbacksDone") });
      })
      .catch((error: unknown) => {
        setStatus({ ok: false, text: errorText(error) });
      })
      .finally(() => {
        clearButton.current?.focus();
      });
  };

  return (
    <section className="settings-section" aria-labelledby="settings-sessions">
      <h4 id="settings-sessions" className="settings-subtitle">
        {t("settings.sessions")}
      </h4>
      <label className="check">
        <input
          type="checkbox"
          checked={settings.restoreSessions}
          aria-describedby="restore-sessions-help"
          onChange={(event) => {
            onChange({ restoreSessions: event.currentTarget.checked });
          }}
        />
        <span>{t("settings.restoreSessions")}</span>
      </label>
      <p id="restore-sessions-help" className="muted settings-help">
        {t("settings.restoreSessionsHelp")}
      </p>
      <p className="muted settings-help">{t("settings.scrollbackSecrets")}</p>
      <fieldset className="settings-group" disabled={!settings.restoreSessions}>
        <legend className="sr-only">{t("settings.scrollbackLines")}</legend>
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
      </fieldset>
      {confirming ? (
        <div className="confirm-box" role="group" aria-labelledby="clear-confirm-text">
          <p id="clear-confirm-text">{t("settings.clearConfirm")}</p>
          <div className="settings-row">
            <button type="button" className="primary-button danger" onClick={clear}>
              {t("settings.clearYes")}
            </button>
            <button
              ref={cancelButton}
              type="button"
              className="secondary-button"
              onClick={() => {
                setConfirming(false);
                clearButton.current?.focus();
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div className="settings-row">
          <button
            ref={clearButton}
            type="button"
            className="secondary-button"
            aria-describedby="clear-scrollbacks-help"
            onClick={() => {
              setStatus(null);
              setConfirming(true);
            }}
          >
            {t("settings.clearScrollbacks")}
          </button>
          <span
            id="clear-scrollbacks-help"
            className={status && !status.ok ? "muted error-text" : "muted"}
            role="status"
          >
            {status ? status.text : t("settings.clearScrollbacksHelp")}
          </span>
        </div>
      )}
    </section>
  );
}

function AppearanceSection(props: Props) {
  const { settings, onChange } = props;
  const t = useT();
  const [layoutReset, setLayoutReset] = useState(false);
  return (
    <>
      <div className="field">
        <span id="settings-theme">{t("settings.theme")}</span>
        <div
          className="segmented"
          role="radiogroup"
          aria-labelledby="settings-theme"
          aria-describedby="settings-theme-help"
        >
          {THEME_SETTINGS.map((theme) => (
            <label key={theme} className="segment">
              <input
                type="radio"
                name="theme"
                value={theme}
                checked={settings.theme === theme}
                onChange={() => {
                  onChange({ theme });
                }}
              />
              <span>{t(THEME_NAMES[theme])}</span>
            </label>
          ))}
        </div>
        <p id="settings-theme-help" className="muted">
          {t("settings.themeHelp")}
        </p>
      </div>
      <RangeField
        id="font-size"
        label={t("settings.fontSize")}
        value={props.fontSize}
        min={FONT_SIZE.min}
        max={FONT_SIZE.max}
        display={t("settings.fontSizeValue", { size: props.fontSize })}
        onChange={(value) => {
          props.onFontSize(clampFontSize(value));
        }}
      />
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button"
          aria-describedby="reset-layout-help"
          onClick={() => {
            props.onResetLayout();
            setLayoutReset(true);
          }}
        >
          {t("settings.resetLayout")}
        </button>
        <span id="reset-layout-help" className="muted" role="status">
          {layoutReset ? t("settings.layoutResetDone") : t("settings.resetLayoutHelp")}
        </span>
      </div>
    </>
  );
}

function AttentionSection({ settings, onChange }: Props) {
  const t = useT();
  const isPreset = ATTENTION_PRESETS.some((preset) => preset.color === settings.attentionColor);
  return (
    <>
      <label className="check">
        <input
          type="checkbox"
          checked={settings.attentionEnabled}
          aria-describedby="attention-help"
          onChange={(event) => {
            onChange({ attentionEnabled: event.currentTarget.checked });
          }}
        />
        <span>{t("settings.attentionEnabled")}</span>
      </label>
      <p id="attention-help" className="muted settings-help">
        {t("settings.attentionHelp")}
      </p>

      <fieldset className="settings-group" disabled={!settings.attentionEnabled}>
        <legend>{t("settings.color")}</legend>
        <div className="swatches">
          {ATTENTION_PRESETS.map((preset) => (
            <label key={preset.id} className="swatch" title={t(COLOR_NAMES[preset.id])}>
              <input
                type="radio"
                name="attention-color"
                value={preset.color}
                aria-label={t(COLOR_NAMES[preset.id])}
                checked={settings.attentionColor === preset.color}
                onChange={() => {
                  onChange({ attentionColor: preset.color });
                }}
              />
              <span className="swatch-color" style={{ background: preset.color }} />
            </label>
          ))}
          <label
            className={isPreset ? "swatch custom" : "swatch custom selected"}
            title={t("settings.customColor")}
          >
            <input
              type="color"
              aria-label={t("settings.customColor")}
              value={settings.attentionColor}
              onChange={(event) => {
                const color = normalizeColor(event.currentTarget.value);
                if (color) onChange({ attentionColor: color });
              }}
            />
          </label>
          <span className="swatch-caption">
            {isPreset ? presetName(settings.attentionColor, t) : t("settings.customColor")}
          </span>
        </div>

        <RangeField
          id="quiet-seconds"
          label={t("settings.quietTime")}
          value={settings.quietSeconds}
          min={QUIET_SECONDS.min}
          max={QUIET_SECONDS.max}
          display={t("settings.seconds", { count: settings.quietSeconds })}
          onChange={(value) => {
            onChange({ quietSeconds: clampQuietSeconds(value) });
          }}
        />

        <label className="check">
          <input
            type="checkbox"
            checked={settings.taskbarFlash}
            aria-describedby="taskbar-flash-help"
            onChange={(event) => {
              onChange({ taskbarFlash: event.currentTarget.checked });
            }}
          />
          <span>{t("settings.taskbarFlash")}</span>
        </label>
        <p id="taskbar-flash-help" className="muted settings-help">
          {t("settings.taskbarFlashHelp")}
        </p>

        <div className="attention-preview" aria-hidden="true">
          <span className="session attention">
            <span className="session-main">
              <span className="badge">C</span>
              <span className="session-text">
                <span className="session-title">{t("settings.preview")}</span>
                <span className="session-meta">
                  <span className="dot attention" />
                  {t("status.attention")}
                </span>
              </span>
            </span>
          </span>
        </div>
      </fieldset>
    </>
  );
}

function ProgramsSection(props: Props) {
  return (
    <>
      <ProgramsSettings
        settings={props.settings}
        profiles={props.profiles}
        versions={props.versions}
        programUpdates={props.programUpdates}
        runningPrograms={props.runningPrograms}
        onRunUpdate={props.onRunUpdate}
        badgeFor={props.badgeFor}
        onChange={props.onChange}
      />
      <ProfileFileField onOpen={props.onOpenProfileFile} onReload={props.onReloadProfiles} />
    </>
  );
}

/** Custom profiles: open profiles.toml and reload it without a restart. */
function ProfileFileField({
  onOpen,
  onReload,
}: {
  onOpen: () => Promise<void>;
  onReload: () => Promise<void>;
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
    <section className="settings-section" aria-labelledby="programs-profile-file">
      <h4 id="programs-profile-file" className="settings-subtitle">
        {t("settings.profileFile")}
      </h4>
      <p id="profile-file-help" className="muted settings-intro">
        {t("settings.profileFileHelp")}
      </p>
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button"
          aria-describedby="profile-file-help"
          disabled={busy}
          onClick={() => {
            run(onOpen, null);
          }}
        >
          {t("settings.openProfileFile")}
        </button>
        <button
          type="button"
          className="secondary-button"
          aria-describedby="profile-file-help"
          disabled={busy}
          onClick={() => {
            run(onReload, t("settings.profilesReloaded"));
          }}
        >
          {t("settings.reloadProfiles")}
        </button>
      </div>
      <p className={status && !status.ok ? "muted error-text" : "muted"} role="status">
        {status?.text ?? ""}
      </p>
    </section>
  );
}

function UpdatesSection({ settings, onChange, update, version, onCheckUpdates }: Props) {
  const t = useT();
  return (
    <>
      {version !== null && (
        <p className="settings-version">
          {/* The placeholder comes last in every language (test in i18n.test.ts). */}
          {t("settings.installedVersion", { version: "" })}
          <VersionText version={version} />
        </p>
      )}
      <label className="check">
        <input
          type="checkbox"
          checked={settings.autoUpdate}
          onChange={(event) => {
            onChange({ autoUpdate: event.currentTarget.checked });
          }}
        />
        <span>{t("settings.autoUpdate")}</span>
      </label>
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button"
          disabled={update.kind === "checking" || update.kind === "installing"}
          onClick={onCheckUpdates}
        >
          {t("settings.checkNow")}
        </button>
        <span className="muted" role="status">
          {updateStatus(update, t)}
        </span>
      </div>
    </>
  );
}

function RangeField({
  id,
  label,
  value,
  min,
  max,
  step = 1,
  display,
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  display: string;
  onChange: (value: number) => void;
}) {
  return (
    <div className="range-field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuetext={display}
        onChange={(event) => {
          onChange(Number(event.currentTarget.value));
        }}
      />
      <output htmlFor={id}>{display}</output>
    </div>
  );
}

function presetName(color: string, t: Translate): string {
  const preset = ATTENTION_PRESETS.find((p) => p.color === color);
  return preset ? t(COLOR_NAMES[preset.id]) : "";
}

function updateStatus(state: UpdateState, t: Translate): string {
  switch (state.kind) {
    case "idle":
      return "";
    case "checking":
      return t("update.checking");
    case "current":
      return t("update.current", { version: displayVersion(state.version, t("version.beta")) });
    case "available":
      return t("update.inSidebar", {
        version: displayVersion(state.update.version, t("version.beta")),
      });
    case "installing":
      return t("update.installing", {
        version: displayVersion(state.update.version, t("version.beta")),
      });
    case "error":
      return state.message;
  }
}
