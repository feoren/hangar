// A sub-agent's figure: one of the bundled figures (src/assets/sub-agents, built by
// tools/figures.py), an own image the backend checked, or the initial on the agent's color.
// Figures stand still: nothing about them moves. The whole figure (`full`) is a portrait box,
// 2 : 3 like the bundled files (tools/figures.py); where there is no room for that, `head` shows
// the face in a round box.

import type { CSSProperties } from "react";

import { useSubAgentLibrary } from "./subAgentLibraryStore";
import { figureFor, runningFigure, type Figure } from "./subAgentLibrary";

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
  /**
   * `full`: the height in CSS pixels, the width is two thirds of it; `head`: the edge of the
   * round face. Missing: the surrounding CSS sets it (a session's look).
   */
  size?: number;
  /** `full`: the whole figure; `head`: the face only, for places too small for the figure. */
  crop?: "head" | "full";
  /** Without a label the figure is decoration next to the agent's name. */
  label?: string;
}

export function AgentFigure({ figure, size, crop = "full", label }: Props) {
  const classes = `agent-figure ${crop === "head" ? "head" : "full"}`;
  const style: CSSProperties =
    size === undefined
      ? {}
      : { width: crop === "head" ? size : Math.round((size * 2) / 3), height: size };
  const a11y =
    label === undefined ? { "aria-hidden": true as const } : { role: "img", "aria-label": label };
  if (figure.kind === "initial") {
    // The letter is as large as half the box's width.
    const fontSize =
      typeof style.width === "number" ? { fontSize: Math.round(style.width * 0.5) } : {};
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
    <span
      className={figure.kind === "image" ? `${classes} custom` : classes}
      style={style}
      {...a11y}
    >
      {url !== undefined && <img src={url} alt="" draggable={false} />}
    </span>
  );
}

/**
 * The figure of a known sub-agent definition by name (`agentType` of a running sub-agent) in a
 * session started in `cwd` (`runningFigure`), `null` when no definition of that name is listed.
 */
export function useKnownFigure(name: string | null, cwd: string | null): Figure | null {
  const { library, figures } = useSubAgentLibrary();
  if (name === null) return null;
  return runningFigure(library, figures, name, cwd);
}

/**
 * The figure of a running sub-agent of type `name` for the crew: a known definition's figure like
 * `useKnownFigure`, else the figure the name alone points to (a built-in type such as `Explore`).
 */
export function useCrewFigure(name: string | null, cwd: string | null): Figure {
  const known = useKnownFigure(name, cwd);
  const { figures } = useSubAgentLibrary();
  return known ?? figureFor(name ?? "agent", null, figures);
}
