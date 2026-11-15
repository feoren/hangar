// Entries of the "Start as sub-agent" menu beside a program that takes one (Claude Code) in the
// sidebar's program list: per definition its face, name, origin and the first line of its
// description. A project definition says so in words; one that overrides a global definition
// of its name wears the neutral figure (subAgentLibrary.ts `defFigure`).

import { AgentFigure } from "./AgentFigure";
import type { Profile } from "./api";
import { folderName } from "./folders";
import type { Translate } from "./i18n";
import type { PopupItem } from "./PopupMenu";
import {
  defFigure,
  firstLine,
  overridesGlobal,
  type AgentDef,
  type FigureView,
  type Library,
} from "./subAgentLibrary";

/** Longest description shown in an entry; the tooltip holds the whole first line. */
const DESCRIPTION_CHARS = 60;

/** Where a definition comes from, in words. */
export function originText(def: AgentDef, library: Library, t: Translate): string {
  return agentOrigin(def, library, t) ?? t("subAgentLib.global");
}

/** A project definition's origin in words ("from project hangar"), `null` for a global one. */
export function agentOrigin(def: AgentDef, library: Library, t: Translate): string | null {
  if (def.scope === "global") return null;
  const folder = folderName(def.folder ?? "");
  const origin = overridesGlobal(library, def)
    ? t("subAgentLib.overridesGlobal", { folder })
    : t("subAgentLib.fromProject", { folder });
  return withExtras(origin, def, t);
}

/**
 * `origin` plus the fields a repository's definition brings beyond its prompt (own hooks, MCP
 * servers, a permission mode), so a start shows them before it runs.
 */
export function withExtras(origin: string, def: AgentDef, t: Translate): string {
  const extras = def.extras ?? [];
  return extras.length === 0
    ? origin
    : `${origin} · ${t("subAgentLib.brings", { fields: extras.join(", ") })}`;
}

export function agentStartItems(
  profile: Profile,
  defs: readonly AgentDef[],
  library: Library,
  figures: ReadonlyMap<string, FigureView>,
  t: Translate,
  start: (profile: Profile, agent: string) => void,
): PopupItem[] {
  const items: PopupItem[] = [
    {
      type: "heading",
      key: "heading",
      label: t("subAgentLib.startHeading", { program: profile.name }),
    },
  ];
  for (const def of defs) {
    const line = firstLine(def.description);
    const short = line.length > DESCRIPTION_CHARS ? `${line.slice(0, DESCRIPTION_CHARS)}…` : line;
    const origin = originText(def, library, t);
    items.push({
      type: "action",
      key: `${def.scope}:${def.folder ?? ""}:${def.name}`,
      label: def.name,
      detail: [origin, short].filter((part) => part !== "").join(" · "),
      title: [t("subAgentLib.startAs", { program: profile.name, agent: def.name }), origin, line]
        .filter((part) => part !== "")
        .join("\n"),
      icon: <AgentFigure figure={defFigure(def, library, figures)} size={22} crop="head" still />,
      run: () => {
        start(profile, def.name);
      },
    });
  }
  return items;
}
