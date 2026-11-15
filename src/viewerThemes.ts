// Color themes of the file viewer window (FileViewerWindow.tsx). Every family has a dark and a light
// variant and follows the app theme, as the terminal schemes do. The values are adapted from
// publicly documented, openly licensed editor palettes (origins and licenses in
// THIRD_PARTY_NOTICES.md); the families carry Hangar's own names, the same as the terminal schemes
// with those colors, so no entry suggests an affiliation with the projects. The viewer and the
// settings preview read them as CSS variables (`viewerCssVariables`) and inline colors; the
// CodeMirror theme in viewerEditor.ts only refers to those variables, so switching never rebuilds
// the editor.

import type { Theme } from "./theme";

/** Order of the picker. Saved values are these keys; a renamed key goes through RENAMED_THEMES. */
export const VIEWER_THEMES = [
  "meridian",
  "harbor",
  "studio",
  "anvil",
  "sundial",
  "velvet",
  "ember",
] as const;
export type ViewerTheme = (typeof VIEWER_THEMES)[number];

export const DEFAULT_VIEWER_THEME: ViewerTheme = "meridian";

/**
 * Keys released in 0.6.0, named after the palettes' origins, and the family each one became in
 * 0.7.0, so a stored choice carries over. Only Ember's colors changed (its palette was redrawn).
 */
const RENAMED_THEMES: Readonly<Record<string, ViewerTheme>> = {
  oneDarkPro: "meridian",
  github: "harbor",
  vscode: "studio",
  darcula: "anvil",
  solarized: "sundial",
  catppuccin: "velvet",
  monokai: "ember",
};

export function isViewerTheme(value: unknown): value is ViewerTheme {
  return VIEWER_THEMES.some((theme) => theme === value);
}

/** A stored viewer theme, renamed keys included; `null` for anything unknown. */
export function parseViewerTheme(value: unknown): ViewerTheme | null {
  if (isViewerTheme(value)) return value;
  if (typeof value !== "string" || !Object.hasOwn(RENAMED_THEMES, value)) return null;
  return RENAMED_THEMES[value] ?? null;
}

/** Kinds of highlighted text; viewerEditor.ts maps them to @lezer/highlight tags. */
export const TOKEN_KINDS = [
  "comment",
  "keyword",
  "control",
  "string",
  "number",
  "constant",
  "function",
  "variable",
  "property",
  "type",
  "operator",
  "tag",
  "attribute",
  "regexp",
  "heading",
  "link",
  "meta",
  "invalid",
] as const;
export type TokenKind = (typeof TOKEN_KINDS)[number];

export interface ViewerPalette {
  background: string;
  foreground: string;
  gutterBackground: string;
  gutterForeground: string;
  /** The number of the line with the cursor. */
  gutterActive: string;
  activeLine: string;
  selection: string;
  cursor: string;
  /** Background of search matches; the current one also gets an outline in `accent`. */
  match: string;
  /** Focus rings, the current search match, the fold placeholder's frame. */
  accent: string;
  tokens: Record<TokenKind, string>;
}

type Tokens = Record<TokenKind, string>;

function palette(
  base: Omit<ViewerPalette, "tokens">,
  tokens: Omit<Tokens, "meta" | "control"> & Partial<Pick<Tokens, "meta" | "control">>,
): ViewerPalette {
  return {
    ...base,
    tokens: {
      ...tokens,
      control: tokens.control ?? tokens.keyword,
      meta: tokens.meta ?? tokens.comment,
    },
  };
}

// Meridian: after the One Dark Pro editor theme and Atom's One Light (both MIT).
const MERIDIAN_DARK = palette(
  {
    background: "#282c34",
    foreground: "#abb2bf",
    gutterBackground: "#282c34",
    gutterForeground: "#636d83",
    gutterActive: "#abb2bf",
    activeLine: "#2c313c",
    selection: "#3e4451",
    cursor: "#528bff",
    match: "#d19a6640",
    accent: "#528bff",
  },
  {
    comment: "#7f848e",
    keyword: "#c678dd",
    string: "#98c379",
    number: "#d19a66",
    constant: "#d19a66",
    function: "#61afef",
    variable: "#e06c75",
    property: "#e06c75",
    type: "#e5c07b",
    operator: "#56b6c2",
    tag: "#e06c75",
    attribute: "#d19a66",
    regexp: "#56b6c2",
    heading: "#e06c75",
    link: "#61afef",
    invalid: "#f44747",
  },
);

const MERIDIAN_LIGHT = palette(
  {
    background: "#fafafa",
    foreground: "#383a42",
    gutterBackground: "#fafafa",
    gutterForeground: "#9d9d9f",
    gutterActive: "#383a42",
    activeLine: "#f0f0f1",
    selection: "#e5e5e6",
    cursor: "#526fff",
    match: "#c1840133",
    accent: "#526fff",
  },
  {
    comment: "#a0a1a7",
    keyword: "#a626a4",
    string: "#50a14f",
    number: "#986801",
    constant: "#986801",
    function: "#4078f2",
    variable: "#e45649",
    property: "#e45649",
    type: "#c18401",
    operator: "#0184bc",
    tag: "#e45649",
    attribute: "#986801",
    regexp: "#0184bc",
    heading: "#e45649",
    link: "#4078f2",
    invalid: "#ca1243",
  },
);

// Harbor: after Primer's github-vscode-theme (MIT).
const HARBOR_DARK = palette(
  {
    background: "#0d1117",
    foreground: "#e6edf3",
    gutterBackground: "#0d1117",
    gutterForeground: "#6e7681",
    gutterActive: "#e6edf3",
    activeLine: "#161b22",
    selection: "#264f78",
    cursor: "#2f81f7",
    match: "#bb800966",
    accent: "#2f81f7",
  },
  {
    comment: "#8b949e",
    keyword: "#ff7b72",
    string: "#a5d6ff",
    number: "#79c0ff",
    constant: "#79c0ff",
    function: "#d2a8ff",
    variable: "#ffa657",
    property: "#79c0ff",
    type: "#ffa657",
    operator: "#ff7b72",
    tag: "#7ee787",
    attribute: "#79c0ff",
    regexp: "#7ee787",
    heading: "#79c0ff",
    link: "#a5d6ff",
    invalid: "#ffa198",
  },
);

const HARBOR_LIGHT = palette(
  {
    background: "#ffffff",
    foreground: "#1f2328",
    gutterBackground: "#ffffff",
    gutterForeground: "#8c959f",
    gutterActive: "#1f2328",
    activeLine: "#f6f8fa",
    selection: "#b6e3ff",
    cursor: "#0969da",
    match: "#fff8c5",
    accent: "#0969da",
  },
  {
    comment: "#6e7781",
    keyword: "#cf222e",
    string: "#0a3069",
    number: "#0550ae",
    constant: "#0550ae",
    function: "#8250df",
    variable: "#953800",
    property: "#0550ae",
    type: "#953800",
    operator: "#cf222e",
    tag: "#116329",
    attribute: "#0550ae",
    regexp: "#116329",
    heading: "#0550ae",
    link: "#0a3069",
    invalid: "#82071e",
  },
);

// Studio: after the Dark+ and Light+ themes of the vscode repository (MIT).
const STUDIO_DARK = palette(
  {
    background: "#1e1e1e",
    foreground: "#d4d4d4",
    gutterBackground: "#1e1e1e",
    gutterForeground: "#858585",
    gutterActive: "#c6c6c6",
    activeLine: "#2a2d2e",
    selection: "#264f78",
    cursor: "#aeafad",
    match: "#623315",
    accent: "#007fd4",
  },
  {
    comment: "#6a9955",
    keyword: "#569cd6",
    control: "#c586c0",
    string: "#ce9178",
    number: "#b5cea8",
    constant: "#569cd6",
    function: "#dcdcaa",
    variable: "#9cdcfe",
    property: "#9cdcfe",
    type: "#4ec9b0",
    operator: "#d4d4d4",
    tag: "#569cd6",
    attribute: "#9cdcfe",
    regexp: "#d16969",
    heading: "#569cd6",
    link: "#3794ff",
    meta: "#c586c0",
    invalid: "#f44747",
  },
);

const STUDIO_LIGHT = palette(
  {
    background: "#ffffff",
    foreground: "#000000",
    gutterBackground: "#ffffff",
    gutterForeground: "#237893",
    gutterActive: "#0b216f",
    activeLine: "#f5f5f5",
    selection: "#add6ff",
    cursor: "#000000",
    match: "#a8ac94",
    accent: "#0090f1",
  },
  {
    comment: "#008000",
    keyword: "#0000ff",
    control: "#af00db",
    string: "#a31515",
    number: "#098658",
    constant: "#0000ff",
    function: "#795e26",
    variable: "#001080",
    property: "#001080",
    type: "#267f99",
    operator: "#000000",
    tag: "#800000",
    attribute: "#e50000",
    regexp: "#811f3f",
    heading: "#800000",
    link: "#0000ff",
    meta: "#af00db",
    invalid: "#cd3131",
  },
);

// Anvil: after the Darcula and light editor schemes of intellij-community (Apache-2.0).
const ANVIL_DARK = palette(
  {
    background: "#2b2b2b",
    foreground: "#a9b7c6",
    gutterBackground: "#313335",
    gutterForeground: "#606366",
    gutterActive: "#a4a3a3",
    activeLine: "#323232",
    selection: "#214283",
    cursor: "#bbbbbb",
    match: "#32593d",
    accent: "#4a88c7",
  },
  {
    comment: "#808080",
    keyword: "#cc7832",
    string: "#6a8759",
    number: "#6897bb",
    constant: "#9876aa",
    function: "#ffc66d",
    variable: "#a9b7c6",
    property: "#9876aa",
    type: "#a9b7c6",
    operator: "#a9b7c6",
    tag: "#e8bf6a",
    attribute: "#bababa",
    regexp: "#6a8759",
    heading: "#cc7832",
    link: "#287bde",
    meta: "#bbb529",
    invalid: "#bc3f3c",
  },
);

const ANVIL_LIGHT = palette(
  {
    background: "#ffffff",
    foreground: "#080808",
    gutterBackground: "#ffffff",
    gutterForeground: "#adadad",
    gutterActive: "#767676",
    activeLine: "#fcfaed",
    selection: "#a6d2ff",
    cursor: "#000000",
    match: "#fbe7a4",
    accent: "#3574f0",
  },
  {
    comment: "#8c8c8c",
    keyword: "#0033b3",
    string: "#067d17",
    number: "#1750eb",
    constant: "#871094",
    function: "#00627a",
    variable: "#080808",
    property: "#871094",
    type: "#080808",
    operator: "#080808",
    tag: "#0033b3",
    attribute: "#174ad4",
    regexp: "#264eff",
    heading: "#0033b3",
    link: "#006dcc",
    meta: "#9e880d",
    invalid: "#f50000",
  },
);

/** Sundial, after Solarized (MIT): accents the same in both variants; only the base tones swap. */
const SUNDIAL_ACCENTS = {
  comment: "#586e75",
  keyword: "#859900",
  string: "#2aa198",
  number: "#d33682",
  constant: "#cb4b16",
  function: "#268bd2",
  property: "#268bd2",
  type: "#b58900",
  tag: "#268bd2",
  attribute: "#b58900",
  regexp: "#dc322f",
  heading: "#cb4b16",
  link: "#6c71c4",
  meta: "#cb4b16",
  invalid: "#dc322f",
};

const SUNDIAL_DARK = palette(
  {
    background: "#002b36",
    foreground: "#839496",
    gutterBackground: "#002b36",
    gutterForeground: "#586e75",
    gutterActive: "#93a1a1",
    activeLine: "#073642",
    selection: "#174652",
    cursor: "#93a1a1",
    match: "#b5890040",
    accent: "#268bd2",
  },
  { ...SUNDIAL_ACCENTS, variable: "#839496", operator: "#839496" },
);

const SUNDIAL_LIGHT = palette(
  {
    background: "#fdf6e3",
    // base01 instead of base00: base00 on base3 stays just below 4.5 : 1.
    foreground: "#586e75",
    gutterBackground: "#fdf6e3",
    gutterForeground: "#93a1a1",
    gutterActive: "#586e75",
    activeLine: "#eee8d5",
    selection: "#e4dcc5",
    cursor: "#586e75",
    match: "#b5890033",
    accent: "#268bd2",
  },
  { ...SUNDIAL_ACCENTS, comment: "#93a1a1", variable: "#586e75", operator: "#586e75" },
);

// Velvet: after the Mocha and Latte flavors of Catppuccin (MIT).
const VELVET_DARK = palette(
  {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    gutterBackground: "#1e1e2e",
    gutterForeground: "#7f849c",
    gutterActive: "#b4befe",
    activeLine: "#2a2b3c",
    selection: "#585b7066",
    cursor: "#f5e0dc",
    match: "#94e2d533",
    accent: "#89b4fa",
  },
  {
    comment: "#9399b2",
    keyword: "#cba6f7",
    string: "#a6e3a1",
    number: "#fab387",
    constant: "#fab387",
    function: "#89b4fa",
    variable: "#cdd6f4",
    property: "#b4befe",
    type: "#f9e2af",
    operator: "#89dceb",
    tag: "#89b4fa",
    attribute: "#f9e2af",
    regexp: "#f5c2e7",
    heading: "#f38ba8",
    link: "#89b4fa",
    meta: "#f5c2e7",
    invalid: "#f38ba8",
  },
);

const VELVET_LIGHT = palette(
  {
    background: "#eff1f5",
    foreground: "#4c4f69",
    gutterBackground: "#eff1f5",
    gutterForeground: "#8c8fa1",
    gutterActive: "#4c4f69",
    activeLine: "#e6e9ef",
    selection: "#7c7f9340",
    cursor: "#dc8a78",
    match: "#17929933",
    accent: "#1e66f5",
  },
  {
    comment: "#7c7f93",
    keyword: "#8839ef",
    string: "#40a02b",
    number: "#fe640b",
    constant: "#fe640b",
    function: "#1e66f5",
    variable: "#4c4f69",
    property: "#7287fd",
    type: "#df8e1d",
    operator: "#04a5e5",
    tag: "#1e66f5",
    attribute: "#df8e1d",
    regexp: "#ea76cb",
    heading: "#d20f39",
    link: "#1e66f5",
    meta: "#ea76cb",
    invalid: "#d20f39",
  },
);

// Ember: after the publicly documented colors of the classic 2006 Monokai TextMate theme, with the
// classic token roles (pink keywords, yellow strings, purple numbers, green functions, cyan types,
// orange for regular expressions).
const EMBER_DARK = palette(
  {
    background: "#272822",
    foreground: "#f8f8f2",
    gutterBackground: "#272822",
    gutterForeground: "#75715e",
    gutterActive: "#cfcfc2",
    activeLine: "#3e3d32",
    selection: "#49483e",
    cursor: "#f8f8f2",
    match: "#e6db7440",
    accent: "#66d9ef",
  },
  {
    comment: "#75715e",
    keyword: "#f92672",
    string: "#e6db74",
    number: "#ae81ff",
    constant: "#ae81ff",
    function: "#a6e22e",
    variable: "#f8f8f2",
    property: "#f8f8f2",
    type: "#66d9ef",
    operator: "#f92672",
    tag: "#f92672",
    attribute: "#a6e22e",
    regexp: "#fd971f",
    heading: "#a6e22e",
    link: "#66d9ef",
    invalid: "#f92672",
  },
);

// Ember's own light variant: the same roles in deepened hues on a warm paper white, matching the
// light terminal scheme.
const EMBER_LIGHT = palette(
  {
    background: "#f9f8f1",
    foreground: "#272822",
    gutterBackground: "#f9f8f1",
    gutterForeground: "#a8a590",
    gutterActive: "#272822",
    activeLine: "#efede0",
    selection: "#e2e0cf",
    cursor: "#272822",
    match: "#8a6f0033",
    accent: "#1e7a96",
  },
  {
    comment: "#75715e",
    keyword: "#c4105a",
    string: "#8a6f00",
    number: "#7346cc",
    constant: "#7346cc",
    function: "#5c7f00",
    variable: "#272822",
    property: "#272822",
    type: "#1e7a96",
    operator: "#c4105a",
    tag: "#c4105a",
    attribute: "#5c7f00",
    regexp: "#c4600a",
    heading: "#5c7f00",
    link: "#1e7a96",
    invalid: "#c4105a",
  },
);

const PALETTES: Record<ViewerTheme, Record<Theme, ViewerPalette>> = {
  meridian: { dark: MERIDIAN_DARK, light: MERIDIAN_LIGHT },
  harbor: { dark: HARBOR_DARK, light: HARBOR_LIGHT },
  studio: { dark: STUDIO_DARK, light: STUDIO_LIGHT },
  anvil: { dark: ANVIL_DARK, light: ANVIL_LIGHT },
  sundial: { dark: SUNDIAL_DARK, light: SUNDIAL_LIGHT },
  velvet: { dark: VELVET_DARK, light: VELVET_LIGHT },
  ember: { dark: EMBER_DARK, light: EMBER_LIGHT },
};

export function viewerPalette(theme: ViewerTheme, mode: Theme): ViewerPalette {
  return PALETTES[theme][mode];
}

/** The palette as CSS custom properties (`--viewer-*`, `--viewer-tok-<kind>`). */
export function viewerCssVariables(palette: ViewerPalette): Record<string, string> {
  const variables: Record<string, string> = {
    "--viewer-bg": palette.background,
    "--viewer-fg": palette.foreground,
    "--viewer-gutter-bg": palette.gutterBackground,
    "--viewer-gutter-fg": palette.gutterForeground,
    "--viewer-gutter-active": palette.gutterActive,
    "--viewer-active-line": palette.activeLine,
    "--viewer-selection": palette.selection,
    "--viewer-cursor": palette.cursor,
    "--viewer-match": palette.match,
    "--viewer-accent": palette.accent,
  };
  for (const kind of TOKEN_KINDS) variables[`--viewer-tok-${kind}`] = palette.tokens[kind];
  return variables;
}

/** One piece of the preview snippet: plain text (`null`) or a highlighted token. */
export type PreviewPiece = readonly [TokenKind | null, string];

/** A few lines of TypeScript for the preview tiles in Settings; decoration only. */
export const PREVIEW_LINES: readonly (readonly PreviewPiece[])[] = [
  [["comment", "// open the file"]],
  [
    ["keyword", "let"],
    [null, " "],
    ["variable", "n"],
    ["operator", " = "],
    ["function", "get"],
    [null, "("],
    ["string", '"x"'],
    [null, ", "],
    ["number", "42"],
    [null, ");"],
  ],
  [
    ["control", "return"],
    [null, " "],
    ["type", "View"],
    [null, "."],
    ["property", "of"],
    [null, "("],
    ["constant", "true"],
    [null, ");"],
  ],
];
