// Terminal color schemes. Every scheme has a dark and a light variant and follows the app theme.
// Apart from Hangar and Dusk, the values are adapted from publicly documented, openly licensed
// palettes (origins and licenses in THIRD_PARTY_NOTICES.md). The schemes carry Hangar's own names,
// so no picker entry suggests an affiliation with the projects the colors come from. Two kinds of
// changes: text colors are raised to readable contrast (foreground 4.5 : 1, every accent and the
// gray of dimmed text 3 : 1, see terminalSchemes.test.ts), and palettes that only exist in one
// variant get a light or dark sibling in the same spirit.

import type { ITheme } from "@xterm/xterm";

import { TERMINAL_THEMES, type AnsiColor, type Theme } from "./theme";

/**
 * Order of the picker: Hangar's own, then the families of the file viewer (same order as there),
 * then popular terminal schemes. Saved values are these keys; a renamed key goes through
 * RENAMED_SCHEMES.
 */
export const COLOR_SCHEMES = [
  "hangar",
  "dusk",
  "meridian",
  "harbor",
  "studio",
  "anvil",
  "sundial",
  "velvet",
  "ember",
  "nocturne",
  "fjord",
  "ochre",
  "neon",
  "heather",
] as const;
export type ColorScheme = (typeof COLOR_SCHEMES)[number];

export const SCHEME_GROUPS = ["own", "editor", "classic"] as const;
export type SchemeGroup = (typeof SCHEME_GROUPS)[number];

const GROUP_OF: Record<ColorScheme, SchemeGroup> = {
  hangar: "own",
  dusk: "own",
  meridian: "editor",
  harbor: "editor",
  studio: "editor",
  anvil: "editor",
  sundial: "editor",
  velvet: "editor",
  ember: "editor",
  nocturne: "classic",
  fjord: "classic",
  ochre: "classic",
  neon: "classic",
  heather: "classic",
};

/** The schemes of a group in picker order. */
export function schemesInGroup(group: SchemeGroup): ColorScheme[] {
  return COLOR_SCHEMES.filter((scheme) => GROUP_OF[scheme] === group);
}

/**
 * Keys released in 0.6.0, named after the palettes' origins, and the scheme each one became in
 * 0.7.0, so a stored choice carries over. Harbor and Meridian, the keys before 0.6.0, are current
 * keys again and hold the same palettes they always had. The colors did not change, except for
 * Ember, whose palette was redrawn.
 */
const RENAMED_SCHEMES: Readonly<Record<string, ColorScheme>> = {
  oneDarkPro: "meridian",
  github: "harbor",
  vscode: "studio",
  darcula: "anvil",
  solarized: "sundial",
  catppuccin: "velvet",
  monokai: "ember",
  dracula: "nocturne",
  nord: "fjord",
  gruvbox: "ochre",
  tokyoNight: "neon",
  rosePine: "heather",
};

export function isColorScheme(value: unknown): value is ColorScheme {
  return COLOR_SCHEMES.some((scheme) => scheme === value);
}

/** A stored scheme, renamed keys included; `null` for anything unknown. */
export function parseColorScheme(value: unknown): ColorScheme | null {
  if (isColorScheme(value)) return value;
  if (typeof value !== "string" || !Object.hasOwn(RENAMED_SCHEMES, value)) return null;
  return RENAMED_SCHEMES[value] ?? null;
}

export type TerminalColors = Required<
  Pick<ITheme, "background" | "foreground" | "cursor" | "cursorAccent" | "selectionBackground">
> &
  Record<AnsiColor, string>;

type Ansi = Record<AnsiColor, string>;

interface Base {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
}

/** The cursor accent is always the background, so the character under a block cursor stays. */
function scheme(base: Base, ansi: Ansi): TerminalColors {
  return { ...base, cursorAccent: base.background, ...ansi };
}

/** Accents as [red, green, yellow, blue, magenta, cyan], once normal and once bright. */
function ansi(
  black: string,
  normal: readonly [string, string, string, string, string, string],
  white: string,
  brightBlack: string,
  bright: readonly [string, string, string, string, string, string],
  brightWhite: string,
): Ansi {
  const [red, green, yellow, blue, magenta, cyan] = normal;
  const [brightRed, brightGreen, brightYellow, brightBlue, brightMagenta, brightCyan] = bright;
  return {
    black,
    red,
    green,
    yellow,
    blue,
    magenta,
    cyan,
    white,
    brightBlack,
    brightRed,
    brightGreen,
    brightYellow,
    brightBlue,
    brightMagenta,
    brightCyan,
    brightWhite,
  };
}

// Dusk: Hangar's softer sibling, a muted slate with less glare. The light variant is Harbor's
// light ink on a faintly gray page.
const DUSK_DARK = scheme(
  {
    background: "#22272e",
    foreground: "#adbac7",
    cursor: "#539bf5",
    selectionBackground: "#2f4866",
  },
  ansi(
    "#545d68",
    ["#f47067", "#57ab5a", "#c69026", "#539bf5", "#b083f0", "#39c5cf"],
    "#909dab",
    "#768390",
    ["#ff938a", "#6bc46d", "#daaa3f", "#6cb6ff", "#dcbdfb", "#56d4dd"],
    "#cdd9e5",
  ),
);

// Harbor: after the terminal colors of Primer's github-vscode-theme (MIT).
const HARBOR_DARK = scheme(
  {
    background: "#0d1117",
    foreground: "#e6edf3",
    cursor: "#2f81f7",
    selectionBackground: "#264f78",
  },
  ansi(
    "#484f58",
    ["#ff7b72", "#3fb950", "#d29922", "#58a6ff", "#bc8cff", "#39c5cf"],
    "#b1bac4",
    "#6e7681",
    ["#ffa198", "#56d364", "#e3b341", "#79c0ff", "#d2a8ff", "#56d4dd"],
    "#ffffff",
  ),
);

const HARBOR_LIGHT = scheme(
  {
    background: "#ffffff",
    foreground: "#1f2328",
    cursor: "#0969da",
    selectionBackground: "#b6e3ff",
  },
  ansi(
    "#24292f",
    ["#cf222e", "#116329", "#4d2d00", "#0969da", "#8250df", "#1b7c83"],
    "#6e7781",
    "#57606a",
    ["#a40e26", "#1a7f37", "#633c01", "#218bff", "#a475f9", "#3192aa"],
    "#8c959f",
  ),
);

const DUSK_LIGHT: TerminalColors = {
  ...HARBOR_LIGHT,
  background: "#f6f8fa",
  cursorAccent: "#f6f8fa",
};

// Meridian: after the terminal colors of the One Dark Pro editor theme and Atom's One Light (both
// MIT). The light brights are darker than the dark accents they come from.
const MERIDIAN_DARK = scheme(
  {
    background: "#282c34",
    foreground: "#abb2bf",
    cursor: "#528bff",
    selectionBackground: "#3e4451",
  },
  ansi(
    "#3f4451",
    ["#e05561", "#8cc265", "#d18f52", "#4aa5f0", "#c162de", "#42b3c2"],
    "#d7dae0",
    "#7f848e",
    ["#ff616e", "#a5e075", "#f0a45d", "#4dc4ff", "#de73ff", "#4cd1e0"],
    "#e6e6e6",
  ),
);

const MERIDIAN_LIGHT = scheme(
  {
    background: "#fafafa",
    foreground: "#383a42",
    cursor: "#526fff",
    selectionBackground: "#dfe1e6",
  },
  ansi(
    "#383a42",
    ["#e45649", "#50a14f", "#986801", "#4078f2", "#a626a4", "#0184bc"],
    "#a0a1a7",
    "#696c77",
    ["#ca1243", "#3d8a3c", "#b07600", "#2f5fd0", "#b53cb3", "#0e7fa6"],
    "#b4b5ba",
  ),
);

// Studio: after the Dark+ and Light+ themes and the integrated terminal defaults of the vscode
// repository (MIT).
const STUDIO_DARK = scheme(
  {
    background: "#1e1e1e",
    foreground: "#cccccc",
    cursor: "#aeafad",
    selectionBackground: "#264f78",
  },
  ansi(
    "#000000",
    ["#d34848", "#0dbc79", "#e5e510", "#498ad1", "#bf47bf", "#11a8cd"],
    "#e5e5e5",
    "#808080",
    ["#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db"],
    "#e5e5e5",
  ),
);

const STUDIO_LIGHT = scheme(
  {
    background: "#ffffff",
    foreground: "#333333",
    cursor: "#000000",
    selectionBackground: "#add6ff",
  },
  ansi(
    "#000000",
    ["#cd3131", "#009c00", "#888c00", "#0451a5", "#bc05bc", "#0592b4"],
    "#555555",
    "#666666",
    ["#cd3131", "#10a910", "#939700", "#0451a5", "#bc05bc", "#0598bc"],
    "#a5a5a5",
  ),
);

// Anvil: after the Darcula console colors of intellij-community (Apache-2.0), with a matching
// light variant.
const ANVIL_DARK = scheme(
  {
    background: "#2b2b2b",
    foreground: "#a9b7c6",
    cursor: "#bbbbbb",
    selectionBackground: "#214283",
  },
  ansi(
    "#000000",
    ["#ff6b68", "#a8c023", "#d6bf55", "#5394ec", "#ae8abe", "#299999"],
    "#999999",
    "#808080",
    ["#ff8785", "#bcd55a", "#ffff00", "#7eaef1", "#ff99ff", "#6cdada"],
    "#ffffff",
  ),
);

const ANVIL_LIGHT = scheme(
  {
    background: "#ffffff",
    foreground: "#080808",
    cursor: "#000000",
    selectionBackground: "#a6d2ff",
  },
  ansi(
    "#000000",
    ["#c7222d", "#067d17", "#9e880d", "#0033b3", "#871094", "#00627a"],
    "#8c8c8c",
    "#737373",
    ["#e3302f", "#1a9c2b", "#a58f0e", "#3574f0", "#a636b8", "#0b8aa3"],
    "#b4b4b4",
  ),
);

// Sundial: after Solarized (MIT). Unlike the original, the bright colors keep their hue: agent
// CLIs use them as accents, and the original grays made gray text invisible.
const SUNDIAL_DARK = scheme(
  {
    background: "#002b36",
    foreground: "#93a1a1",
    cursor: "#93a1a1",
    selectionBackground: "#174652",
  },
  ansi(
    "#073642",
    ["#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198"],
    "#eee8d5",
    "#657b83",
    ["#cb4b16", "#859900", "#b58900", "#268bd2", "#6c71c4", "#2aa198"],
    "#fdf6e3",
  ),
);

const SUNDIAL_LIGHT = scheme(
  {
    background: "#fdf6e3",
    foreground: "#586e75",
    cursor: "#586e75",
    selectionBackground: "#e6dfc9",
  },
  ansi(
    "#073642",
    ["#dc322f", "#829600", "#b18600", "#268bd2", "#d33682", "#299c93"],
    "#93a1a1",
    "#657b83",
    ["#cb4b16", "#829600", "#b18600", "#268bd2", "#6c71c4", "#299c93"],
    "#586e75",
  ),
);

// Velvet: after the Mocha and Latte flavors of Catppuccin (MIT).
const VELVET_DARK = scheme(
  {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#f5e0dc",
    selectionBackground: "#45475a",
  },
  ansi(
    "#45475a",
    ["#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5"],
    "#bac2de",
    "#7f849c",
    ["#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5"],
    "#a6adc8",
  ),
);

const VELVET_LIGHT = scheme(
  {
    background: "#eff1f5",
    foreground: "#4c4f69",
    cursor: "#dc8a78",
    selectionBackground: "#ccd0da",
  },
  ansi(
    "#5c5f77",
    ["#d20f39", "#3e9b2a", "#be7919", "#1e66f5", "#c965af", "#179299"],
    "#acb0be",
    "#6c6f85",
    ["#d20f39", "#3e9b2a", "#be7919", "#1e66f5", "#c965af", "#179299"],
    "#bcc0cc",
  ),
);

// Ember: after the publicly documented colors of the classic 2006 Monokai TextMate theme. The
// classic theme has no blue, so blue is its cyan and cyan a lighter, greener tint of it; the
// brights are lighter steps of the same hues, and the comment gray is raised for dimmed text.
const EMBER_DARK = scheme(
  {
    background: "#272822",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "#49483e",
  },
  ansi(
    "#3e3d32",
    ["#f92672", "#a6e22e", "#e6db74", "#66d9ef", "#ae81ff", "#a1efe4"],
    "#cfcfc2",
    "#8f8b76",
    ["#ff5c95", "#bdf35a", "#f2ea9b", "#8ee5f5", "#c4a5ff", "#c4f7ef"],
    "#f8f8f2",
  ),
);

// Ember's own light variant: the same hues, deepened until they read on a warm paper white.
const EMBER_LIGHT = scheme(
  {
    background: "#f9f8f1",
    foreground: "#272822",
    cursor: "#272822",
    selectionBackground: "#e2e0cf",
  },
  ansi(
    "#272822",
    ["#c4105a", "#5c7f00", "#8a6f00", "#1e7a96", "#7346cc", "#11806f"],
    "#8a8775",
    "#6b6853",
    ["#d62a6c", "#6b9400", "#9e7f00", "#2a8bab", "#8a5ce6", "#14917f"],
    "#b8b5a2",
  ),
);

// Nocturne: after the Dracula theme (MIT), and its light variant.
const NOCTURNE_DARK = scheme(
  {
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "#44475a",
  },
  ansi(
    "#21222c",
    ["#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd"],
    "#f8f8f2",
    "#6d7caa",
    ["#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff"],
    "#ffffff",
  ),
);

const NOCTURNE_LIGHT = scheme(
  {
    background: "#fffbeb",
    foreground: "#1f1f1f",
    cursor: "#1f1f1f",
    selectionBackground: "#cfcfde",
  },
  ansi(
    "#1f1f1f",
    ["#cb3a2a", "#14710a", "#846e15", "#644ac9", "#a3144d", "#036a96"],
    "#9a9580",
    "#6c664b",
    ["#d74c3d", "#198d0c", "#9b8318", "#7862d0", "#bf185a", "#047fb4"],
    "#bcb8a3",
  ),
);

// Fjord: after Nord (MIT). Red and the comment gray are raised a little on the dark background;
// the light variant sets the accents in deeper tones on the palette's lightest gray.
const FJORD_DARK = scheme(
  {
    background: "#2e3440",
    foreground: "#d8dee9",
    cursor: "#d8dee9",
    selectionBackground: "#434c5e",
  },
  ansi(
    "#3b4252",
    ["#c36c74", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0"],
    "#e5e9f0",
    "#7b88a1",
    ["#c36c74", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb"],
    "#eceff4",
  ),
);

const FJORD_LIGHT = scheme(
  {
    background: "#eceff4",
    foreground: "#2e3440",
    cursor: "#2e3440",
    selectionBackground: "#d8dee9",
  },
  ansi(
    "#3b4252",
    ["#bf616a", "#5f7d4b", "#9a7424", "#5e81ac", "#8f6a88", "#3f7f8f"],
    "#7b88a1",
    "#616e88",
    ["#a8434c", "#4f6f37", "#8a6a1f", "#4c6d96", "#7d5876", "#356f7d"],
    "#aeb7c6",
  ),
);

// Ochre: after gruvbox (MIT/X11).
const OCHRE_DARK = scheme(
  {
    background: "#282828",
    foreground: "#ebdbb2",
    cursor: "#ebdbb2",
    selectionBackground: "#504945",
  },
  ansi(
    "#3c3836",
    ["#d23c36", "#98971a", "#d79921", "#589194", "#b16286", "#689d6a"],
    "#a89984",
    "#928374",
    ["#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c"],
    "#ebdbb2",
  ),
);

const OCHRE_LIGHT = scheme(
  {
    background: "#fbf1c7",
    foreground: "#3c3836",
    cursor: "#3c3836",
    selectionBackground: "#d5c4a1",
  },
  ansi(
    "#3c3836",
    ["#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#427b58"],
    "#7c6f64",
    "#928374",
    ["#cc241d", "#8f8e18", "#b27f1b", "#458588", "#b16286", "#639565"],
    "#a89984",
  ),
);

// Neon: after the Night and Day styles of tokyonight.nvim (Apache-2.0).
const NEON_DARK = scheme(
  {
    background: "#1a1b26",
    foreground: "#c0caf5",
    cursor: "#c0caf5",
    selectionBackground: "#283457",
  },
  ansi(
    "#15161e",
    ["#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff"],
    "#a9b1d6",
    "#6a7297",
    ["#ff899d", "#9fe044", "#faba4a", "#8db0ff", "#c7a9ff", "#a4daff"],
    "#c0caf5",
  ),
);

const NEON_LIGHT = scheme(
  {
    background: "#e1e2e7",
    foreground: "#3760bf",
    cursor: "#3760bf",
    selectionBackground: "#b7c1e3",
  },
  ansi(
    "#b4b5b9",
    ["#e6275f", "#587539", "#8c6c3e", "#2c78e0", "#9854f1", "#007197"],
    "#6172b0",
    "#6f7388",
    ["#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#007197"],
    "#3760bf",
  ),
);

// Heather: after the Main and Dawn variants of Rosé Pine (MIT).
const HEATHER_DARK = scheme(
  {
    background: "#191724",
    foreground: "#e0def4",
    cursor: "#524f67",
    selectionBackground: "#403d52",
  },
  ansi(
    "#26233a",
    ["#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba"],
    "#e0def4",
    "#6e6a86",
    ["#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba"],
    "#e0def4",
  ),
);

const HEATHER_LIGHT = scheme(
  {
    background: "#faf4ed",
    foreground: "#575279",
    cursor: "#9893a5",
    selectionBackground: "#dfdad9",
  },
  ansi(
    "#575279",
    ["#b4637a", "#286983", "#be7f2a", "#56949f", "#907aa9", "#c47673"],
    "#9893a5",
    "#797593",
    ["#b4637a", "#286983", "#be7f2a", "#56949f", "#907aa9", "#c47673"],
    "#bdb7be",
  ),
);

const SCHEMES: Record<ColorScheme, Record<Theme, TerminalColors>> = {
  hangar: TERMINAL_THEMES,
  dusk: { dark: DUSK_DARK, light: DUSK_LIGHT },
  meridian: { dark: MERIDIAN_DARK, light: MERIDIAN_LIGHT },
  harbor: { dark: HARBOR_DARK, light: HARBOR_LIGHT },
  studio: { dark: STUDIO_DARK, light: STUDIO_LIGHT },
  anvil: { dark: ANVIL_DARK, light: ANVIL_LIGHT },
  sundial: { dark: SUNDIAL_DARK, light: SUNDIAL_LIGHT },
  velvet: { dark: VELVET_DARK, light: VELVET_LIGHT },
  ember: { dark: EMBER_DARK, light: EMBER_LIGHT },
  nocturne: { dark: NOCTURNE_DARK, light: NOCTURNE_LIGHT },
  fjord: { dark: FJORD_DARK, light: FJORD_LIGHT },
  ochre: { dark: OCHRE_DARK, light: OCHRE_LIGHT },
  neon: { dark: NEON_DARK, light: NEON_LIGHT },
  heather: { dark: HEATHER_DARK, light: HEATHER_LIGHT },
};

/** Always the same object per scheme and theme, so xterm only repaints on a real change. */
export function schemeColors(scheme: ColorScheme, theme: Theme): TerminalColors {
  return SCHEMES[scheme][theme];
}

/** The accents a preview shows for a scheme, in the usual terminal order. */
export function schemePalette(colors: TerminalColors): string[] {
  return [colors.red, colors.green, colors.yellow, colors.blue, colors.magenta, colors.cyan];
}

/** Search hits: every match, and the current one. `#rrggbb` only (xterm's search addon). */
export interface SearchColors {
  match: string;
  active: string;
}

const AMBER_LIGHT: SearchColors = { match: "#fff1b8", active: "#f5c84c" };

const SEARCH_COLORS: Record<ColorScheme, Record<Theme, SearchColors>> = {
  hangar: { dark: { match: "#4a3d14", active: "#8a6416" }, light: AMBER_LIGHT },
  dusk: { dark: { match: "#463b1a", active: "#6c5414" }, light: AMBER_LIGHT },
  meridian: {
    dark: { match: "#45402a", active: "#6b5520" },
    light: { match: "#f6e7b0", active: "#ecc35a" },
  },
  harbor: { dark: { match: "#4a3d14", active: "#8a6416" }, light: AMBER_LIGHT },
  studio: { dark: { match: "#4a3d14", active: "#7a5a12" }, light: AMBER_LIGHT },
  anvil: {
    dark: { match: "#32593d", active: "#5a4a14" },
    light: { match: "#fbe7a4", active: "#f2c94c" },
  },
  sundial: {
    dark: { match: "#2f4a2a", active: "#5c4d07" },
    light: { match: "#f0e2a8", active: "#e2c45c" },
  },
  velvet: {
    dark: { match: "#4d4636", active: "#8a7346" },
    light: { match: "#f6e3bd", active: "#efc072" },
  },
  ember: {
    dark: { match: "#5a5236", active: "#7d6a2c" },
    light: { match: "#f6e2bd", active: "#eec27a" },
  },
  nocturne: {
    dark: { match: "#4c4a3a", active: "#6e6a3c" },
    light: { match: "#f6ecb4", active: "#ead77a" },
  },
  fjord: {
    dark: { match: "#4c4c4a", active: "#6e6346" },
    light: { match: "#f3e6c2", active: "#e8cf8a" },
  },
  ochre: {
    dark: { match: "#4a3f1e", active: "#76601c" },
    light: { match: "#f2dc92", active: "#e9c252" },
  },
  neon: {
    dark: { match: "#3d3a35", active: "#5e4f33" },
    light: { match: "#f0e2b8", active: "#e6c878" },
  },
  heather: {
    dark: { match: "#3e3240", active: "#5f4a40" },
    light: { match: "#f6e2c2", active: "#f0c88a" },
  },
};

export function searchColors(scheme: ColorScheme, theme: Theme): SearchColors {
  return SEARCH_COLORS[scheme][theme];
}
