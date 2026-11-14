import { useEffect, useState } from "react";

import { api } from "./api";
import {
  fewerRules,
  GUARDED_PROGRAMS,
  parseGuardInfo,
  problemPlace,
  shippedRules,
  type GuardInfo,
} from "./commandGuard";
import { useT } from "./i18n";

/**
 * The command guard's switch under the gate setting: on by default, only effective while the gate
 * is (team sessions). Below it the rules file to edit and, if any, what in it could not be read;
 * the backend reads the file again on every look, so an edit shows up when Settings opens.
 */
export function CommandGuardSettings(props: {
  enabled: boolean;
  /** The gate is on: without it no session sends its commands. */
  available: boolean;
  onChange: (enabled: boolean) => void;
}) {
  const t = useT();
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
  }, [props.enabled]);

  return (
    <>
      <label className={props.available ? "check" : "check disabled"}>
        <input
          type="checkbox"
          checked={props.enabled && props.available}
          disabled={!props.available}
          aria-describedby="command-guard-help"
          onChange={(event) => {
            props.onChange(event.currentTarget.checked);
          }}
        />
        <span>{t("settings.commandGuard")}</span>
      </label>
      <div id="command-guard-help" className="muted settings-help">
        <p>{t("settings.commandGuardHelp", { programs: GUARDED_PROGRAMS })}</p>
        {info !== null && info.file !== "" && (
          <p className="selectable command-guard-file">
            {t("settings.commandGuardFile", { file: info.file })}
          </p>
        )}
        {info !== null && info.defaults && (
          <p className="command-guard-problem">{t("settings.commandGuardDefaults")}</p>
        )}
        {info !== null && fewerRules(info) && (
          <p className="command-guard-problem">
            {t("settings.commandGuardFewer", {
              count: shippedRules(info),
              defaults: info.defaultCount,
            })}
          </p>
        )}
        {info !== null && info.problems.length > 0 && (
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
    </>
  );
}
