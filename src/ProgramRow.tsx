// One program in "New session": the whole row starts a session with the program's default, and a
// quiet "Model" pill beside it (ModelMenu.tsx) starts one with a chosen model. The two are sibling
// buttons, so neither sits inside the other. Anything below the row (an update offer) comes in as
// children and wraps onto its own line.

import type { ReactNode } from "react";

import type { SessionModel } from "./api";
import { Badge } from "./Badge";
import { useT } from "./i18n";
import type { ModelSection } from "./localLlm";
import { ModelMenuButton } from "./ModelMenu";
import type { BadgeLook } from "./programPrefs";

export function ProgramRow(props: {
  name: string;
  /** The command line, shown in the tooltip for those who want to know what runs. */
  commandLine: string;
  badge: BadgeLook;
  /** The installed version, or null to leave it out. */
  version: string | null;
  updateAvailable: boolean;
  /** Empty: the program offers no model choice and the row shows no pill. */
  models: readonly ModelSection[];
  onStart: (model?: SessionModel) => void;
  /** A model was picked from the menu: the caller decides where the focus goes. */
  onMenuClose: () => void;
  children?: ReactNode;
}) {
  const t = useT();
  const label = t("sidebar.startProgram", { program: props.name });
  return (
    <li className="profile-row">
      <button
        type="button"
        className="profile"
        aria-label={label}
        title={`${label}\n${props.commandLine}`}
        onClick={() => {
          props.onStart();
        }}
      >
        <Badge look={props.badge} />
        <span className="profile-name">{props.name}</span>
        {props.version !== null && (
          <span className="profile-version">
            <span aria-hidden="true">·</span> {props.version}
          </span>
        )}
        {props.updateAvailable && (
          <span className="dot update profile-update-dot" aria-hidden="true" />
        )}
      </button>
      {props.models.length > 0 && (
        <ModelMenuButton
          program={props.name}
          sections={props.models}
          onPick={(model) => {
            props.onStart(model);
          }}
          onAction={props.onMenuClose}
        />
      )}
      {props.children}
    </li>
  );
}
