// Icons of the catalog agents, bundled with the app (src/assets/agent-icons, one `<id>.svg` per
// catalog id; sources and licenses in NOTICE.md there). They are tiles with the brand mark or a
// monogram, so every agent looks the same whether its CLI came as an .exe with an icon or as an
// npm/pip script without one. Shipped at build time; nothing is fetched at runtime.

const FILES = import.meta.glob<string>("./assets/agent-icons/*.svg", {
  eager: true,
  query: "?url",
  import: "default",
});

function idOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1, -".svg".length);
}

const ICONS: ReadonlyMap<string, string> = new Map(
  Object.entries(FILES).map(([path, url]) => [idOf(path), url]),
);

/** IDs that have a bundled icon. */
export const AGENT_ICON_IDS: readonly string[] = [...ICONS.keys()];

/** URL of the bundled icon for a catalog id, `undefined` for shells and the user's own programs. */
export function agentIcon(id: string): string | undefined {
  return ICONS.get(id);
}
