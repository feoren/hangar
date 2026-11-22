// The bundled figure files as URLs: the still figures (src/assets/sub-agents, 240 x 360, built by
// tools/figures.py) and their calm idle loops (src/assets/sub-agents-animated, animated WebP,
// 160 x 240, the figure in the same place as in its still file). Only the URLs are part of the
// script; Vite emits each file as its own asset, so a file is read when an image first shows it,
// never at the start. Any subset of the figures may have a loop; one without stays still.

const FILES = import.meta.glob<string>("./assets/sub-agents/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

const ANIMATED_FILES = import.meta.glob<string>("./assets/sub-agents-animated/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

function byId(files: Record<string, string>): ReadonlyMap<string, string> {
  return new Map(
    Object.entries(files).map(([path, url]) => [
      path.slice(path.lastIndexOf("/") + 1, -".webp".length),
      url,
    ]),
  );
}

const URLS = byId(FILES);
const ANIMATED_URLS = byId(ANIMATED_FILES);

/** Ids of the bundled figure files (kept equal to `BUILTIN_FIGURES` by a test). */
export const BUNDLED_FIGURES: readonly string[] = [...URLS.keys()].sort();

export function builtinFigureUrl(id: string): string | undefined {
  return URLS.get(id);
}

/** Ids of the bundled figures that have an animated file (a test keeps them built-in ids). */
export const ANIMATED_FIGURES: readonly string[] = [...ANIMATED_URLS.keys()].sort();

export function animatedFigureUrl(id: string): string | undefined {
  return ANIMATED_URLS.get(id);
}
