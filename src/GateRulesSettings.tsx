import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

import { api, GATE_CHANGED } from "./api";
import {
  parseGateRules,
  parseGateSnapshot,
  ruleAnswerText,
  rulePathLabel,
  type GateRule,
  type RuleAnswer,
} from "./gate";
import { useT } from "./i18n";
import { TrashIcon } from "./icons";
import { revealInvisible } from "./invisible";

/** Entries of the log shown in Settings; the backend keeps more. */
const SHOWN_LOG = 10;

function timeOf(at: number, language: string): string {
  return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(
    new Date(at),
  );
}

/**
 * The gate's remembered answers (gate_rules.rs) under the gate setting: each rule with its project,
 * a delete button, and what the rules answered lately. Shows nothing while there is neither a rule
 * nor an automatic answer, so the setting stays compact until the card's "Remember for this
 * project" was used once.
 */
export function GateRulesSettings(props: { language: string }) {
  const t = useT();
  const [rules, setRules] = useState<GateRule[]>([]);
  const [log, setLog] = useState<RuleAnswer[]>([]);

  const refresh = useCallback(() => {
    api
      .gateRules()
      .then((value) => {
        setRules(parseGateRules(value));
      })
      .catch(() => undefined);
    api
      .gateSnapshot()
      .then((value) => {
        const snapshot = parseGateSnapshot(value);
        if (snapshot !== null) setLog(snapshot.log);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refresh();
    let disposed = false;
    let stop: (() => void) | null = null;
    try {
      // A card answered with "Remember" adds a rule and sends a snapshot: both lists follow it.
      listen(GATE_CHANGED, refresh)
        .then((unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there are no backend events.
    }
    return () => {
      disposed = true;
      stop?.();
    };
  }, [refresh]);

  if (rules.length === 0 && log.length === 0) return null;

  const remove = (rule: GateRule) => {
    setRules((current) => current.filter((entry) => entry.id !== rule.id));
    api
      .deleteGateRule(rule.id)
      .catch(() => undefined)
      .finally(refresh);
  };

  return (
    <div className="field">
      <span id="gate-rules-title">{t("settings.gateRules")}</span>
      <p className="muted settings-help">
        {rules.length === 0 ? t("settings.gateRulesEmpty") : t("settings.gateRulesHelp")}
      </p>
      {rules.length > 0 && (
        <ul className="gate-rule-list" aria-labelledby="gate-rules-title">
          {rules.map((rule) => {
            const path = rulePathLabel(rule.path, rule.scope);
            return (
              <li key={rule.id} className="gate-rule-item">
                <span className={`gate-rule-effect ${rule.effect}`}>
                  {t(rule.effect === "allow" ? "settings.gateRuleAllow" : "settings.gateRuleDeny")}
                </span>
                <span className="gate-rule-path selectable" title={path}>
                  {path}
                </span>
                <span className="gate-rule-project">{revealInvisible(rule.projectName)}</span>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={t("settings.gateRuleDelete", { path })}
                  title={t("settings.gateRuleDelete", { path })}
                  onClick={() => {
                    remove(rule);
                  }}
                >
                  <TrashIcon />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {log.length > 0 && (
        <>
          <span id="gate-log-title" className="muted">
            {t("settings.gateLog")}
          </span>
          <ul className="gate-log" aria-labelledby="gate-log-title">
            {log.slice(0, SHOWN_LOG).map((entry) => {
              const text = `${entry.session}: ${ruleAnswerText(entry, t)}`;
              return (
                <li key={entry.id} className="gate-log-item">
                  <span className="gate-log-time">{timeOf(entry.at, props.language)}</span>
                  <span className="gate-log-text selectable" title={text}>
                    {text}
                  </span>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
