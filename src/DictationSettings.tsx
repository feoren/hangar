// Settings section "Dictation": on/off, the speech model (download only after consent, with size
// and source named), the language, and what happens to the audio.

import { useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import {
  DICTATION_LANGUAGES,
  DICTATION_MODELS,
  type DictationLanguage,
  type DictationModel,
  type DictationPrefs,
  type DictationState,
} from "./dictation";
import { formatBytes } from "./format";
import { useT, type MessageKey } from "./i18n";
import type { Settings } from "./settings";
import { DICTATION_SHORTCUT, formatShortcut } from "./shortcuts";
import { useDictationStatus } from "./useDictation";

interface Props {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}

const MODEL_NAMES: Record<DictationModel, MessageKey> = {
  base: "dictation.modelBase",
  small: "dictation.modelSmall",
};

const MODEL_HELP: Record<DictationModel, MessageKey> = {
  base: "dictation.modelBaseHelp",
  small: "dictation.modelSmallHelp",
};

const LANGUAGE_NAMES: Record<DictationLanguage, MessageKey> = {
  auto: "dictation.languageAuto",
  de: "dictation.languageDe",
  en: "dictation.languageEn",
};

const ERROR_TEXT: Record<string, MessageKey> = {
  download: "dictation.downloadFailed",
  storage: "dictation.storageFailed",
  busy: "dictation.busy",
};

function stateText(state: DictationState): MessageKey {
  switch (state.kind) {
    case "notInstalled":
      return "dictation.stateNotInstalled";
    case "downloading":
      return "dictation.stateDownloading";
    case "ready":
      return "dictation.stateReady";
    case "recording":
      return "dictation.recording";
    case "transcribing":
      return "dictation.transcribing";
    case "noMicrophone":
      return "dictation.noMicrophone";
    case "permissionDenied":
      return "dictation.permissionDenied";
    case "error":
      return "dictation.failed";
  }
}

export function DictationSettings({ settings, onChange }: Props) {
  const t = useT();
  const prefs = settings.dictation;
  const status = useDictationStatus(prefs.model);
  const [consent, setConsent] = useState<DictationModel | null>(null);
  const [problem, setProblem] = useState<MessageKey | null>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const language = settings.language;

  useEffect(() => {
    if (consent !== null) cancelButton.current?.focus();
  }, [consent]);

  const change = (patch: Partial<DictationPrefs>) => {
    onChange({ dictation: { ...prefs, ...patch } });
  };
  const fail = (error: unknown) => {
    setProblem(ERROR_TEXT[errorText(error)] ?? "dictation.failed");
  };
  const install = (model: DictationModel) => {
    setConsent(null);
    setProblem(null);
    change({ model });
    api.dictationInstallModel(model).catch(fail);
  };
  const remove = (model: DictationModel) => {
    setProblem(null);
    api.dictationRemoveModel(model).catch(fail);
  };

  const downloading = status.state.kind === "downloading" ? status.state : null;
  const busy =
    downloading !== null ||
    status.state.kind === "recording" ||
    status.state.kind === "transcribing";
  const sizeOf = (model: DictationModel) => {
    const bytes = status.models.find((info) => info.id === model)?.bytes;
    return bytes === undefined ? "" : formatBytes(bytes, language);
  };
  const shortcut = formatShortcut(DICTATION_SHORTCUT, t);
  // Without an input device a download would only fill the disk: the section says so instead.
  const noMicrophone = !status.microphone;

  return (
    <>
      <section className="settings-section" aria-labelledby="settings-dictation-use">
        <h4 id="settings-dictation-use" className="settings-subtitle">
          {t("dictation.use")}
        </h4>
        <label className="check">
          <input
            type="checkbox"
            checked={settings.dictationEnabled}
            aria-describedby="dictation-enabled-help"
            onChange={(event) => {
              onChange({ dictationEnabled: event.currentTarget.checked });
            }}
          />
          <span>{t("dictation.enabled")}</span>
        </label>
        <p id="dictation-enabled-help" className="muted settings-help">
          {t("dictation.enabledHelp", { shortcut })}
        </p>
        <p className="muted settings-help">{t("dictation.privacy")}</p>
      </section>

      <section className="settings-section" aria-labelledby="settings-dictation-model">
        <h4 id="settings-dictation-model" className="settings-subtitle">
          {t("dictation.model")}
        </h4>
        <div className="dictation-model-list" role="radiogroup" aria-label={t("dictation.model")}>
          {DICTATION_MODELS.map((model) => {
            const info = status.models.find((entry) => entry.id === model);
            const installed = info?.installed === true;
            const selected = prefs.model === model;
            const loading = downloading?.model === model ? downloading : null;
            return (
              <div key={model} className="dictation-model" data-selected={selected}>
                <label>
                  <input
                    type="radio"
                    name="dictation-model"
                    checked={selected}
                    onChange={() => {
                      change({ model });
                    }}
                  />
                  <span>
                    {t(MODEL_NAMES[model])} <span className="muted">({sizeOf(model)})</span>
                    <br />
                    <span className="muted">{t(MODEL_HELP[model])}</span>
                  </span>
                </label>
                <div className="dictation-model-actions">
                  {loading !== null ? (
                    <>
                      <progress
                        className="dictation-progress"
                        max={1}
                        value={loading.progress}
                        aria-label={t("dictation.stateDownloading")}
                      />
                      <span className="muted">
                        {t("dictation.percent", {
                          count: String(Math.round(loading.progress * 100)),
                        })}
                      </span>
                    </>
                  ) : installed ? (
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={busy}
                      onClick={() => {
                        remove(model);
                      }}
                    >
                      {t("dictation.remove")}
                    </button>
                  ) : noMicrophone ? null : (
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={busy}
                      onClick={() => {
                        setProblem(null);
                        setConsent(model);
                      }}
                    >
                      {t("dictation.download")}
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
        {noMicrophone && <p className="settings-help">{t("dictation.noMicrophoneSetup")}</p>}
        {consent !== null && !noMicrophone && (
          <div
            className="confirm-box"
            role="alertdialog"
            aria-labelledby="dictation-consent-title"
            aria-describedby="dictation-consent-text"
          >
            <p id="dictation-consent-title">
              <strong>
                {t("dictation.consentTitle", {
                  name: t(MODEL_NAMES[consent]),
                  size: sizeOf(consent),
                })}
              </strong>
            </p>
            <p id="dictation-consent-text" className="muted">
              {t("dictation.consentBody", { source: status.source })}
            </p>
            <div className="settings-row">
              <button
                type="button"
                className="primary-button"
                onClick={() => {
                  install(consent);
                }}
              >
                {t("dictation.consentYes")}
              </button>
              <button
                ref={cancelButton}
                type="button"
                className="secondary-button"
                onClick={() => {
                  setConsent(null);
                }}
              >
                {t("common.cancel")}
              </button>
            </div>
          </div>
        )}
        <p className={problem ? "muted error-text" : "muted"} role="status">
          {problem ? t(problem) : t(stateText(status.state))}
        </p>
      </section>

      <section className="settings-section" aria-labelledby="settings-dictation-language">
        <h4 id="settings-dictation-language" className="settings-subtitle">
          {t("dictation.language")}
        </h4>
        <div className="field">
          <label htmlFor="dictation-language" className="sr-only">
            {t("dictation.language")}
          </label>
          <select
            id="dictation-language"
            className="select"
            value={prefs.language}
            aria-describedby="dictation-language-help"
            onChange={(event) => {
              const value = event.currentTarget.value;
              const next = DICTATION_LANGUAGES.find((entry) => entry === value);
              if (next !== undefined) change({ language: next });
            }}
          >
            {DICTATION_LANGUAGES.map((entry) => (
              <option key={entry} value={entry}>
                {t(LANGUAGE_NAMES[entry])}
              </option>
            ))}
          </select>
          <p id="dictation-language-help" className="muted">
            {t("dictation.languageHelp")}
          </p>
        </div>
      </section>
    </>
  );
}
