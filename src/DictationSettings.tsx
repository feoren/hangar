// Settings section "Dictation": one switch; while it is on, the key and the language, then the
// speech models (download only after consent, with size and source named), each with its status.

import { useEffect, useId, useRef, useState } from "react";

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
import { InlineConfirm, SettingRow, SettingsGroup, SwitchRow } from "./SettingsParts";
import { DICTATION_KEYS, dictationKeyName } from "./shortcuts";
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

/** A failed download or removal, shown at the card of the model it concerned. */
interface Problem {
  model: DictationModel;
  text: MessageKey;
}

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
  const [problem, setProblem] = useState<Problem | null>(null);
  const modelTitle = useId();
  const downloadButtons = useRef(new Map<DictationModel, HTMLButtonElement>());
  const choices = useRef(new Map<DictationModel, HTMLInputElement>());
  /**
   * Where the focus goes once the question at a card closes: back to its download button after
   * "Cancel"; to the model's choice after "Download", since the button gives way to the progress.
   */
  const refocus = useRef<{ model: DictationModel; to: "download" | "choice" } | null>(null);
  const language = settings.language;

  useEffect(() => {
    const target = refocus.current;
    if (consent !== null || target === null) return;
    const map = target.to === "download" ? downloadButtons.current : choices.current;
    map.get(target.model)?.focus();
    refocus.current = null;
  }, [consent]);

  const change = (patch: Partial<DictationPrefs>) => {
    onChange({ dictation: { ...prefs, ...patch } });
  };
  const fail = (model: DictationModel) => (error: unknown) => {
    setProblem({ model, text: ERROR_TEXT[errorText(error)] ?? "dictation.failed" });
  };
  const install = (model: DictationModel) => {
    setConsent(null);
    setProblem(null);
    change({ model });
    api.dictationInstallModel(model).catch(fail(model));
  };
  const remove = (model: DictationModel) => {
    setProblem(null);
    api.dictationRemoveModel(model).catch(fail(model));
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
  const shortcut = dictationKeyName(prefs.key, t, true);
  // Without an input device a download would only fill the disk: the chosen card says so instead.
  const noMicrophone = !status.microphone;
  /** The line under a model card: a failure there, else the state of the chosen model. */
  const cardStatus = (model: DictationModel): { text: string; error: boolean } | null => {
    if (problem?.model === model) return { text: t(problem.text), error: true };
    if (prefs.model !== model) return null;
    if (noMicrophone) return { text: t("dictation.noMicrophoneSetup"), error: false };
    // The progress bar on the card already says it.
    if (downloading?.model === model) return null;
    return { text: t(stateText(status.state)), error: false };
  };

  return (
    <>
      <SettingsGroup>
        <SwitchRow
          label={t("dictation.enabled")}
          help={t("dictation.enabledHelp", { shortcut })}
          checked={settings.dictationEnabled}
          onChange={(dictationEnabled) => {
            onChange({ dictationEnabled });
          }}
        >
          <SettingRow
            label={t("dictation.key")}
            control={({ labelId }) => (
              <select
                id="dictation-key"
                className="select"
                value={prefs.key}
                aria-labelledby={labelId}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  const next = DICTATION_KEYS.find((entry) => entry === value);
                  if (next !== undefined) change({ key: next });
                }}
              >
                {DICTATION_KEYS.map((entry) => (
                  <option key={entry} value={entry}>
                    {dictationKeyName(entry, t)}
                  </option>
                ))}
              </select>
            )}
          />
          <SettingRow
            label={t("dictation.language")}
            help={t("dictation.languageHelp")}
            control={({ labelId, helpId }) => (
              <select
                id="dictation-language"
                className="select"
                value={prefs.language}
                aria-labelledby={labelId}
                aria-describedby={helpId}
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
            )}
          />
        </SwitchRow>
      </SettingsGroup>

      {settings.dictationEnabled && (
        // The cards stand on their own, without the group frame: a frame around frames is one too many.
        <section className="setting-group" aria-labelledby={modelTitle}>
          <h4 id={modelTitle} className="setting-group-title">
            {t("dictation.model")}
          </h4>
          <div className="dictation-model-list" role="radiogroup" aria-labelledby={modelTitle}>
            {DICTATION_MODELS.map((model) => {
              const info = status.models.find((entry) => entry.id === model);
              const installed = info?.installed === true;
              const selected = prefs.model === model;
              const loading = downloading?.model === model ? downloading : null;
              const line = cardStatus(model);
              return (
                <div key={model} className="dictation-model" data-selected={selected}>
                  <label>
                    <input
                      ref={(element) => {
                        if (element) choices.current.set(model, element);
                        else choices.current.delete(model);
                      }}
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
                    ) : noMicrophone || consent === model ? null : (
                      <button
                        ref={(element) => {
                          if (element) downloadButtons.current.set(model, element);
                          else downloadButtons.current.delete(model);
                        }}
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
                  {consent === model && !noMicrophone && (
                    <InlineConfirm
                      danger={false}
                      text={
                        <>
                          <strong>
                            {t("dictation.consentTitle", {
                              name: t(MODEL_NAMES[model]),
                              size: sizeOf(model),
                            })}
                          </strong>
                          <br />
                          <span className="muted">
                            {t("dictation.consentBody", { source: status.source })}
                          </span>
                        </>
                      }
                      confirm={t("dictation.consentYes")}
                      onConfirm={() => {
                        refocus.current = { model, to: "choice" };
                        install(model);
                      }}
                      onCancel={() => {
                        refocus.current = { model, to: "download" };
                        setConsent(null);
                      }}
                    />
                  )}
                  {/* Always there, so a change is read out; empty on a card it does not concern. */}
                  <p
                    className={
                      line?.error === true
                        ? "dictation-model-status setting-help error-text"
                        : "dictation-model-status setting-help"
                    }
                    role="status"
                  >
                    {line?.text ?? ""}
                  </p>
                </div>
              );
            })}
          </div>
        </section>
      )}
    </>
  );
}
