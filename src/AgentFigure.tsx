// A sub-agent's figure: one of the bundled figures (src/assets/sub-agents, built by
// tools/figures.py), an own image the backend checked, or the initial on the agent's color. The
// idle motion is CSS only (styles.css, `.agent-figure`): it stops for reduced motion and in the
// power saving mode, and grows while the sub-agent works.

import type { CSSProperties } from "react";

import { useSubAgentLibrary } from "./subAgentLibraryStore";
import { defFigure, overridesGlobal, type Figure } from "./subAgentLibrary";

const FILES = import.meta.glob<string>("./assets/sub-agents/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

const URLS: ReadonlyMap<string, string> = new Map(
  Object.entries(FILES).map(([path, url]) => [
    path.slice(path.lastIndexOf("/") + 1, -".webp".length),
    url,
  ]),
);

/** Ids of the bundled figure files (kept equal to `BUILTIN_FIGURES` by a test). */
export const BUNDLED_FIGURES: readonly string[] = [...URLS.keys()].sort();

export function builtinFigureUrl(id: string): string | undefined {
  return URLS.get(id);
}

interface Props {
  figure: Figure;
  /** Edge length in CSS pixels; missing: the surrounding CSS sets it (a session's look). */
  size?: number;
  /** `head`: the face only, for small places; `full`: the whole figure. */
  crop?: "head" | "full";
  /** Moves a little more, while the sub-agent works. */
  busy?: boolean;
  /** Without a label the figure is decoration next to the agent's name. */
  label?: string;
  /** Still, without the idle motion (a picker's choices). */
  still?: boolean;
}

export function AgentFigure({ figure, size, crop = "full", busy = false, label, still }: Props) {
  const classes = [
    "agent-figure",
    crop === "head" ? "head" : "full",
    busy ? "busy" : "",
    still === true ? "still" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const style: CSSProperties = size === undefined ? {} : { width: size, height: size };
  const a11y =
    label === undefined ? { "aria-hidden": true as const } : { role: "img", "aria-label": label };
  if (figure.kind === "initial") {
    const fontSize = size === undefined ? {} : { fontSize: Math.round(size * 0.5) };
    return (
      <span
        className={`${classes} initial`}
        style={{ ...style, ...fontSize, background: figure.color }}
        {...a11y}
      >
        {figure.letter}
      </span>
    );
  }
  const url = figure.kind === "builtin" ? builtinFigureUrl(figure.id) : figure.url;
  return (
    <span className={classes} style={style} {...a11y}>
      {url !== undefined && <img src={url} alt="" draggable={false} />}
    </span>
  );
}

/**
 * The figure of a known sub-agent definition by name (`agentType` of a running sub-agent), `null`
 * when no definition of that name is listed.
 */
export function useKnownFigure(name: string | null): Figure | null {
  const { library, figures } = useSubAgentLibrary();
  if (name === null) return null;
  const defs = library.defs.filter((entry) => entry.name === name);
  // The tree knows no folder: a project file that overrides the global definition anywhere
  // keeps the global figure off, so a repository's agent never wears the trusted one.
  const def = defs.find((entry) => overridesGlobal(library, entry)) ?? defs[0];
  return def === undefined ? null : defFigure(def, library, figures);
}
