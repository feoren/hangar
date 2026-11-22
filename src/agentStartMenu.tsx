// A sub-agent definition's origin in words, for the sub-agent settings and the start palette: a
// project definition says so, one that overrides a global definition of its name says that too,
// and what it brings beyond its prompt is named.

import { folderName } from "./folders";
import type { Translate } from "./i18n";
import { overridesGlobal, type AgentDef, type Library } from "./subAgentLibrary";

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
