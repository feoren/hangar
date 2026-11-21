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

/** The gate's remembered answers (gate_rules.rs) and what they answered lately, kept current. */
export function useGateRules(): {
  rules: GateRule[];
  log: RuleAnswer[];
  remove: (rule: GateRule) => void;
} {
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
      // A card answered with "Remember" adds a rule and signals a change: both lists follow it.
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

  const remove = (rule: GateRule) => {
    setRules((current) => current.filter((entry) => entry.id !== rule.id));
    api
      .deleteGateRule(rule.id)
      .catch(() => undefined)
      .finally(refresh);
  };

  return { rules, log, remove };
}

/** Each remembered answer with its project and a delete button. */
export function GateRuleList({
  rules,
  label,
  onRemove,
}: {
  rules: readonly GateRule[];
  label: string;
  onRemove: (rule: GateRule) => void;
}) {
  const t = useT();
  return (
    <ul className="gate-rule-list" aria-label={label}>
      {rules.map((rule) => {
        const path = rulePathLabel(rule.path, rule.scope, t);
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
                onRemove(rule);
              }}
            >
              <TrashIcon />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** What the remembered answers answered lately, newest first. */
export function GateLog({ log, language }: { log: readonly RuleAnswer[]; language: string }) {
  const t = useT();
  return (
    <div className="gate-log-block">
      <span id="gate-log-title" className="muted">
        {t("settings.gateLog")}
      </span>
      <ul className="gate-log" aria-labelledby="gate-log-title">
        {log.slice(0, SHOWN_LOG).map((entry) => {
          const text = `${entry.session}: ${ruleAnswerText(entry, t)}`;
          return (
            <li key={entry.id} className="gate-log-item">
              <span className="gate-log-time">{timeOf(entry.at, language)}</span>
              <span className="gate-log-text selectable" title={text}>
                {text}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
