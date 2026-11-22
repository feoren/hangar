// The model field of Settings > Sub-agents, for one definition or a program's default: Claude
// Code's aliases and "another id"; Codex CLI's model (free text with suggestions) and effort.
// A value the file holds that is not offered is shown as it is and kept until changed.

import { useId, useState } from "react";

import { useT, type MessageKey } from "./i18n";
import {
  claudeOptions,
  effortOptions,
  modelName,
  OTHER_MODEL,
  validModel,
  type DefProgram,
  type ModelChoice,
} from "./subAgentModels";

const EFFORT_TEXTS: Readonly<Record<string, MessageKey>> = {
  low: "subAgentLib.effort.low",
  medium: "subAgentLib.effort.medium",
  high: "subAgentLib.effort.high",
  xhigh: "subAgentLib.effort.xhigh",
  max: "subAgentLib.effort.max",
};

export function ModelField({
  program,
  value,
  suggestions = [],
  label,
  onChange,
}: {
  program: DefProgram;
  value: ModelChoice;
  suggestions?: readonly string[];
  /** Names what the field is for, for screen readers ("Model of backend"). */
  label: string;
  onChange: (choice: ModelChoice) => void;
}) {
  return program === "claude" ? (
    <ClaudeField value={value} label={label} onChange={onChange} />
  ) : (
    <CodexField value={value} suggestions={suggestions} label={label} onChange={onChange} />
  );
}

function ClaudeField({
  value,
  label,
  onChange,
}: {
  value: ModelChoice;
  label: string;
  onChange: (choice: ModelChoice) => void;
}) {
  const t = useT();
  const [other, setOther] = useState<string | null>(null);
  const options = claudeOptions(value.model);
  const optionText = (option: string) => {
    if (option === "") return t("subAgentLib.modelSession");
    if (option === OTHER_MODEL) return t("subAgentLib.modelOther");
    if (option === "fable") return t("subAgentLib.modelFable");
    return modelName(option);
  };
  return (
    <span className="model-field">
      <select
        className="select compact"
        aria-label={label}
        value={other === null ? (value.model ?? "") : OTHER_MODEL}
        onChange={(event) => {
          const next = event.currentTarget.value;
          if (next === OTHER_MODEL) {
            setOther(value.model ?? "");
            return;
          }
          setOther(null);
          onChange({ model: next === "" ? null : next, effort: null });
        }}
      >
        {options.map((option) => (
          <option key={option} value={option}>
            {optionText(option)}
          </option>
        ))}
      </select>
      {other !== null && (
        <ModelText
          initial={other}
          label={t("subAgentLib.modelOtherLabel")}
          onCommit={(model) => {
            setOther(null);
            onChange({ model, effort: null });
          }}
          onCancel={() => {
            setOther(null);
          }}
        />
      )}
    </span>
  );
}

/** A model id typed in: taken with Enter or the button when valid, dropped with Escape. */
function ModelText({
  initial,
  label,
  suggestions = [],
  allowEmpty = false,
  onCommit,
  onCancel,
}: {
  initial: string;
  label: string;
  suggestions?: readonly string[];
  /** An empty field means "the session's model". */
  allowEmpty?: boolean;
  onCommit: (model: string | null) => void;
  onCancel?: () => void;
}) {
  const t = useT();
  const list = useId();
  const [text, setText] = useState(initial);
  const trimmed = text.trim();
  const valid = trimmed === "" ? allowEmpty : validModel(trimmed);
  // An empty field is not yet wrong: the hint waits until something is typed.
  const wrong = !valid && trimmed !== "";
  const commit = () => {
    if (!valid) return;
    onCommit(trimmed === "" ? null : trimmed);
  };
  return (
    <span className="model-text">
      <input
        className="text-input mono compact"
        aria-label={label}
        aria-invalid={wrong}
        placeholder={allowEmpty ? t("subAgentLib.modelSession") : undefined}
        list={suggestions.length > 0 ? list : undefined}
        spellCheck={false}
        maxLength={64}
        value={text}
        onChange={(event) => {
          setText(event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
          if (event.key === "Escape" && onCancel !== undefined) onCancel();
        }}
      />
      {suggestions.length > 0 && (
        <datalist id={list}>
          {suggestions.map((model) => (
            <option key={model} value={model} />
          ))}
        </datalist>
      )}
      {trimmed !== initial.trim() && (
        <button
          type="button"
          className="secondary-button compact"
          disabled={!valid}
          onClick={commit}
        >
          {t("subAgentLib.modelApply")}
        </button>
      )}
      {wrong && <span className="muted error-text">{t("subAgentLib.modelInvalid")}</span>}
    </span>
  );
}

function CodexField({
  value,
  suggestions,
  label,
  onChange,
}: {
  value: ModelChoice;
  suggestions: readonly string[];
  label: string;
  onChange: (choice: ModelChoice) => void;
}) {
  const t = useT();
  return (
    <span className="model-field">
      <ModelText
        // A new value from the backend starts the field afresh.
        key={value.model ?? ""}
        initial={value.model ?? ""}
        label={label}
        suggestions={suggestions}
        allowEmpty
        onCommit={(model) => {
          onChange({ model, effort: value.effort });
        }}
      />
      <select
        className="select compact"
        aria-label={t("subAgentLib.effortLabel")}
        title={t("subAgentLib.effortLabel")}
        value={value.effort ?? ""}
        onChange={(event) => {
          const effort = event.currentTarget.value;
          onChange({ model: value.model, effort: effort === "" ? null : effort });
        }}
      >
        {effortOptions(value.effort).map((effort) => {
          const key = EFFORT_TEXTS[effort];
          return (
            <option key={effort} value={effort}>
              {effort === "" ? t("subAgentLib.effortSession") : key === undefined ? effort : t(key)}
            </option>
          );
        })}
      </select>
    </span>
  );
}
