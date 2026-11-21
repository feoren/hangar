import { useEffect, useState } from "react";

import { api } from "./api";
import {
  fewerRules,
  parseGuardInfo,
  problemPlace,
  shippedRules,
  type GuardInfo,
} from "./commandGuard";
import { useT } from "./i18n";

/**
 * The command guard's rules file as the backend reads it: the path, whether the shipped rules
 * stand in, and what in the file could not be read. The backend reads the file again on every
 * look, so an edit shows up when Settings opens or the switch changes. `null` until known.
 */
export function useCommandGuardInfo(enabled: boolean): GuardInfo | null {
  const [info, setInfo] = useState<GuardInfo | null>(null);
  useEffect(() => {
    let current = true;
    api
      .commandGuardInfo()
      .then((value) => {
        if (current) setInfo(parseGuardInfo(value));
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [enabled]);
  return info;
}

/** Whether the rules file needs a look: unreadable, rules missing, or lines with problems. */
export function guardNeedsAttention(info: GuardInfo | null): boolean {
  return info !== null && (info.defaults || fewerRules(info) || info.problems.length > 0);
}

/** Where the rules file lies and, if any, what in it could not be read. */
export function CommandGuardFile({ info }: { info: GuardInfo }) {
  const t = useT();
  return (
    <div className="command-guard-details">
      {info.file !== "" && (
        <p className="selectable command-guard-file">
          {t("settings.commandGuardFile", { file: info.file })}
        </p>
      )}
      {info.defaults && (
        <p className="command-guard-problem">{t("settings.commandGuardDefaults")}</p>
      )}
      {fewerRules(info) && (
        <p className="command-guard-problem">
          {t("settings.commandGuardFewer", {
            count: shippedRules(info),
            defaults: info.defaultCount,
          })}
        </p>
      )}
      {info.problems.length > 0 && (
        <ul className="command-guard-problems" aria-label={t("settings.commandGuardProblems")}>
          {info.problems.map((problem, index) => {
            const place = problemPlace(problem, t);
            return (
              <li key={index} className="command-guard-problem selectable">
                {place === null ? problem.message : `${place}: ${problem.message}`}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
