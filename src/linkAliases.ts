// Member names for a new link group (ADR 0014): short, readable names the agents call each other
// by with `hangar-send <name>`. A session's title is often its first prompt, which makes a poor
// name ("ich-habe-so-viele-agenten-nu"), so the window picks the names from what stays stable: the
// project folder, or the program where several members share a folder. The backend checks them
// again (links.rs `checked_name`). Pure.

import { ALL_MEMBERS, LEAD_ALIAS, validMemberName } from "./links";
import { projectFolder } from "./sessionLabel";

/** Longest name the window derives, before a "-2" suffix; the backend allows up to 32. */
export const ALIAS_MAX = 20;
/** Name of a member that gives nothing usable. */
const FALLBACK = "agent";

/** Letters that spell out in ASCII the way the backend's `base_name` does it. */
const SPELLED: Readonly<Record<string, string>> = {
  ä: "ae",
  æ: "ae",
  ö: "oe",
  œ: "oe",
  ü: "ue",
  ß: "ss",
};

/**
 * `text` as a name: lowercase ASCII letters, digits and single dashes, accented Latin letters
 * spelled out, cut to `ALIAS_MAX` at a dash where one is near, never inside a word unless the
 * first word alone is longer. Empty when nothing usable is left.
 */
export function aliasSlug(text: string): string {
  let slug = "";
  for (const char of text.toLowerCase()) {
    if (/[a-z0-9]/.test(char)) {
      slug += char;
      continue;
    }
    const spelled =
      SPELLED[char] ??
      char
        .normalize("NFD")
        .replace(/[\u0300-\u036f]/g, "")
        .replace(/[^a-z0-9]/g, "");
    if (spelled !== "") slug += spelled;
    else if (slug !== "" && !slug.endsWith("-")) slug += "-";
  }
  slug = slug.replace(/-+$/, "");
  if (slug.length <= ALIAS_MAX) return slug;
  const head = slug.slice(0, ALIAS_MAX + 1);
  const dash = head.lastIndexOf("-");
  // A first word longer than the limit is cut hard; anything else ends at a word.
  return dash > 0 ? head.slice(0, dash) : slug.slice(0, ALIAS_MAX);
}

/** What a new member's name is made of. */
export interface AliasSource {
  /** Path of the project folder: the repository's root for a worktree session. */
  path: string;
  /** The program's display name ("Claude Code"). */
  program: string;
}

/** A member already in the group: where it works, and the name it keeps. */
export interface NamedMember extends AliasSource {
  name: string;
}

/**
 * Names for new members, in order. Each takes its project folder ("hangar"); members that share
 * a folder take their program instead ("claude-code", "codex"), and what is still taken gets a
 * number ("codex-2"). Never "all" or "lead", always a valid member name. `members`: those already
 * in the group the newcomers join; their folders count as shared and their names as taken.
 */
export function memberAliases(
  sources: readonly AliasSource[],
  members: readonly NamedMember[] = [],
): string[] {
  const folders = sources.map((source) => aliasSlug(projectFolder(source.path)));
  const all = [...folders, ...members.map((member) => aliasSlug(projectFolder(member.path)))];
  const shared = (slug: string) => all.filter((other) => other === slug).length > 1;
  const names: string[] = members.map((member) => member.name);
  sources.forEach((source, index) => {
    const folder = folders[index] ?? "";
    const program = aliasSlug(source.program);
    const base =
      folder !== "" && !shared(folder) ? folder : program !== "" ? program : folder || FALLBACK;
    const taken = (name: string) =>
      name === ALL_MEMBERS || name === LEAD_ALIAS || names.includes(name);
    let name = validMemberName(base) && !taken(base) ? base : "";
    for (let n = 2; name === ""; n += 1) {
      const numbered = `${base}-${String(n)}`;
      if (!taken(numbered)) name = numbered;
    }
    names.push(name);
  });
  return names.slice(members.length);
}
