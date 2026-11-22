import { useEffect, useId, useRef, useState, useSyncExternalStore, type ReactNode } from "react";

import { AccountSettings } from "./AccountSettings";
import type { Profile } from "./api";
import { Dialog } from "./Dialog";
import { DialogResizeHandles, useDialogSize, type DialogSizeBounds } from "./DialogResize";
import { GeneralSettings } from "./GeneralSettings";
import { useT, type MessageKey } from "./i18n";
import { CheckIcon, CloseIcon } from "./icons";
import { clampFontSize, FONT_SIZE } from "./layout";
import { availableProfiles, type BadgeLook } from "./programPrefs";
import type { ProgramVersions } from "./programs";
import type { UpdateActions } from "./ProgramUpdateOffer";
import { AgentInstructionsGroup, ProgramsAdvanced } from "./ProgramFilesSettings";
import { AvailableList, ProgramsSettings } from "./ProgramsSettings";
import { AttentionSettings } from "./AttentionSettings";
import { RangeField } from "./RangeField";
import { RowLayoutSetting } from "./RowLayoutSettings";
import {
  ActionRow,
  SegmentedField,
  SettingRow,
  SettingsGroup,
  Switch,
  SwitchRow,
  WideRow,
} from "./SettingsParts";
import { treeFlowSupported } from "./treeFlow";
import { TreeFlowField } from "./TreeFlowSettings";
import type { Settings } from "./settings";
import { SecuritySettings } from "./SecuritySettings";
import { DictationSettings } from "./DictationSettings";
import { SnippetsSettings } from "./SnippetsSettings";
import { SubAgentSettings } from "./SubAgentSettings";
import {
  nextTabIndex,
  SECTION_NAMES,
  SETTINGS_SECTIONS,
  type SettingsSection,
} from "./settingsSections";
import {
  canvasMeasure,
  CURSOR_STYLES,
  FONT_SAMPLE,
  fontChoices,
  fontStack,
  installedFonts,
  schemeColors,
  type ColorScheme,
  type CursorStyle,
  type SystemFont,
  type TerminalColors,
} from "./terminalLook";
import {
  PREVIEW_LINES as TERMINAL_PREVIEW_LINES,
  TILE_LINES,
  type PreviewLine,
} from "./terminalPreview";
import { SCHEME_GROUPS, schemesInGroup, type SchemeGroup } from "./terminalSchemes";
import { SESSION_GROUPINGS, type SessionGrouping } from "./sessionGroups";
import { THEME_SETTINGS, type AnsiColor, type Theme, type ThemeSetting } from "./theme";
import {
  PREVIEW_LINES,
  VIEWER_THEMES,
  viewerPalette,
  type ViewerPalette,
  type ViewerTheme,
} from "./viewerThemes";
import type { ProgramUpdateState } from "./useProgramUpdates";
import type { UsageConsent } from "./usageStats";
import type { UpdateState } from "./useUpdater";
import { useScrollToTop } from "./useScrollToTop";
import { hangarUpdateStatus, programUpdateStatus } from "./updateStatus";
import { VersionText } from "./VersionText";

interface Props {
  /** The section to open on; `null`: the one chosen last. */
  initialSection: SettingsSection | null;
  settings: Settings;
  profiles: Profile[];
  versions: ProgramVersions;
  badgeFor: (profileId: string) => BadgeLook;
  fontSize: number;
  /** Appearance shown right now (the resolved one for "System"), for the terminal preview. */
  theme: Theme;
  update: UpdateState;
  /** Time of Hangar's last successful update check (ms), `null` before the first. */
  updateCheckedAt: number | null;
  /** Version of Hangar, `null` until known. */
  version: string | null;
  programUpdates: ProgramUpdateState;
  /** "Update" on a program row (one click, or a question while the program runs). */
  updateActions: UpdateActions;
  onChange: (patch: Partial<Settings>) => void;
  /** Deletes all saved scrollback; rejects with a readable error. */
  onClearScrollbacks: () => Promise<void>;
  /** Opens profiles.toml in the default editor (creates it if needed); rejects with an error. */
  onOpenProfileFile: () => Promise<void>;
  /** Opens HANGAR.md in the default editor (creates it if needed); rejects with an error. */
  onOpenHangarMd: () => Promise<void>;
  /** Rereads profiles.toml; rejects with the backend's error (profileFile.ts). */
  onReloadProfiles: () => Promise<void>;
  /** A profile was saved or deleted in the editor: apply the backend's new list. */
  onProfiles: (profiles: Profile[]) => void;
  onFontSize: (size: number) => void;
  onResetLayout: () => void;
  onCheckUpdates: () => void;
  /** The answer about the anonymous usage statistics; `undefined` while it is being read. */
  usageConsent: UsageConsent | undefined;
  onUsageConsent: (enabled: boolean) => void;
  onOpenUsageInfo: () => void;
  onClose: () => void;
}

/** Proper names, the same in every language. */
const SCHEME_NAMES: Record<ColorScheme, MessageKey> = {
  hangar: "scheme.hangar",
  dusk: "scheme.dusk",
  meridian: "scheme.meridian",
  harbor: "scheme.harbor",
  studio: "scheme.studio",
  anvil: "scheme.anvil",
  sundial: "scheme.sundial",
  velvet: "scheme.velvet",
  ember: "scheme.ember",
  nocturne: "scheme.nocturne",
  fjord: "scheme.fjord",
  ochre: "scheme.ochre",
  neon: "scheme.neon",
  heather: "scheme.heather",
};

const SCHEME_GROUP_NAMES: Record<SchemeGroup, MessageKey> = {
  own: "settings.schemeGroupOwn",
  editor: "settings.schemeGroupEditor",
  classic: "settings.schemeGroupClassic",
};

/** Proper names, the same in every language. */
const VIEWER_THEME_NAMES: Record<ViewerTheme, MessageKey> = {
  meridian: "viewerTheme.meridian",
  harbor: "viewerTheme.harbor",
  studio: "viewerTheme.studio",
  anvil: "viewerTheme.anvil",
  sundial: "viewerTheme.sundial",
  velvet: "viewerTheme.velvet",
  ember: "viewerTheme.ember",
};

const CURSOR_NAMES: Record<CursorStyle, MessageKey> = {
  block: "settings.cursorBlock",
  bar: "settings.cursorBar",
  underline: "settings.cursorUnderline",
};

/** Installed system fonts, measured once per app start (fonts rarely change while Hangar runs). */
let fontCache: readonly SystemFont[] | null = null;

function availableFonts(): readonly SystemFont[] {
  if (fontCache === null) {
    const measure = canvasMeasure();
    fontCache = measure === null ? [] : installedFonts(measure);
  }
  return fontCache;
}

const THEME_NAMES: Record<ThemeSetting, MessageKey> = {
  system: "settings.themeSystem",
  light: "settings.themeLight",
  dark: "settings.themeDark",
};

const GROUPING_NAMES: Record<SessionGrouping, MessageKey> = {
  program: "sidebar.groupByProgram",
  project: "sidebar.groupByProject",
};

/** Same breakpoint as in styles.css: below it, the sections sit in a row above the content. */
const NARROW_QUERY = "(max-width: 620px)";

function subscribeNarrow(onChange: () => void): () => void {
  const query = window.matchMedia(NARROW_QUERY);
  query.addEventListener("change", onChange);
  return () => {
    query.removeEventListener("change", onChange);
  };
}

function isNarrow(): boolean {
  return window.matchMedia(NARROW_QUERY).matches;
}

/** Most recently chosen section while Hangar runs. After a restart it starts at General. */
let lastSection: SettingsSection = "general";

/**
 * The standard size matches `--large-dialog-width` and `--large-dialog-height` in styles.css. The
 * minimum keeps the section list and a field row side by side; a smaller window still wins.
 */
const SIZE_BOUNDS: DialogSizeBounds = {
  storageKey: "hangar.settingsDialogSize",
  standard: { width: 936, height: 672 },
  min: { width: 640, height: 420 },
};

/**
 * Sections as vertical tabs on the left (WAI-ARIA Tabs, automatic activation), content on the
 * right. Every change takes effect immediately and is saved right away, so there is neither a
 * Save button nor a second Close button: the X in the header closes, as does Escape.
 */
export function SettingsDialog(props: Props) {
  const t = useT();
  const [section, setSection] = useState<SettingsSection>(props.initialSection ?? lastSection);
  const tabs = useRef(new Map<SettingsSection, HTMLButtonElement>());
  const panel = useRef<HTMLDivElement>(null);
  const narrow = useSyncExternalStore(subscribeNarrow, isNarrow);
  const size = useDialogSize(SIZE_BOUNDS);

  useEffect(() => {
    lastSection = section;
  }, [section]);
  // Every section opens at its top, also when the dialog opens on the last one.
  useScrollToTop(panel, section);

  const select = (next: SettingsSection, focus: boolean) => {
    setSection(next);
    if (focus) tabs.current.get(next)?.focus();
  };

  const onTabKey = (event: React.KeyboardEvent) => {
    const index = SETTINGS_SECTIONS.indexOf(section);
    const next = nextTabIndex(event.key, index, SETTINGS_SECTIONS.length);
    const target = next === null ? undefined : SETTINGS_SECTIONS[next];
    if (target === undefined) return;
    event.preventDefault();
    select(target, true);
  };

  return (
    <Dialog
      labelledBy="settings-title"
      className="settings resizable"
      style={size.style}
      onClose={props.onClose}
    >
      <div className="dialog-header">
        <h2 id="settings-title">{t("settings.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="settings-body">
        <div
          className="settings-tabs"
          role="tablist"
          aria-orientation={narrow ? "horizontal" : "vertical"}
          aria-label={t("settings.sections")}
          onKeyDown={onTabKey}
        >
          {SETTINGS_SECTIONS.map((id) => {
            const selected = id === section;
            return (
              <button
                key={id}
                ref={(element) => {
                  if (element) tabs.current.set(id, element);
                  else tabs.current.delete(id);
                }}
                type="button"
                role="tab"
                id={`settings-tab-${id}`}
                className="settings-tab"
                aria-selected={selected}
                aria-controls={selected ? `settings-panel-${id}` : undefined}
                tabIndex={selected ? 0 : -1}
                data-autofocus={selected ? true : undefined}
                onClick={() => {
                  select(id, false);
                }}
              >
                {t(SECTION_NAMES[id])}
              </button>
            );
          })}
        </div>

        <div
          ref={panel}
          className="settings-panel"
          role="tabpanel"
          id={`settings-panel-${section}`}
          aria-labelledby={`settings-tab-${section}`}
          tabIndex={0}
        >
          <h3 className="settings-panel-title">{t(SECTION_NAMES[section])}</h3>
          <SectionContent section={section} {...props} />
        </div>
      </div>

      <DialogResizeHandles state={size} />
    </Dialog>
  );
}

function SectionContent(props: Props & { section: SettingsSection }) {
  switch (props.section) {
    case "general":
      return (
        <GeneralSettings
          settings={props.settings}
          onChange={props.onChange}
          onClearScrollbacks={props.onClearScrollbacks}
          usageConsent={props.usageConsent}
          onUsageConsent={props.onUsageConsent}
          onOpenUsageInfo={props.onOpenUsageInfo}
        />
      );
    case "security":
      return <SecuritySettings settings={props.settings} onChange={props.onChange} />;
    case "appearance":
      return <AppearanceSection {...props} />;
    case "attention":
      return <AttentionSettings {...props} />;
    case "programs":
      return <ProgramsSection {...props} />;
    case "subAgents":
      return <SubAgentSettings settings={props.settings} onChange={props.onChange} />;
    case "snippets":
      return (
        <SnippetsSettings
          snippets={props.settings.snippets}
          onChange={(snippets) => {
            props.onChange({ snippets });
          }}
        />
      );
    case "dictation":
      return <DictationSettings settings={props.settings} onChange={props.onChange} />;
    case "account":
      return <AccountSettings />;
    case "updates":
      return <UpdatesSection {...props} />;
  }
}

/**
 * Appearance, most used first: the app's look, the terminals, the sidebar and the file viewer.
 * Resetting the layout sits last as a quiet link: it only puts sizes back.
 */
function AppearanceSection(props: Props) {
  const { settings, onChange } = props;
  const t = useT();
  return (
    <>
      <SettingsGroup title={t("settings.groupInterface")}>
        <SegmentedField
          label={t("settings.theme")}
          help={t("settings.themeHelp")}
          value={settings.theme}
          options={THEME_SETTINGS.map((theme) => ({ value: theme, label: t(THEME_NAMES[theme]) }))}
          onChange={(theme) => {
            onChange({ theme });
          }}
        />
      </SettingsGroup>
      <TerminalLookGroup {...props} />
      <SettingsGroup title={t("settings.groupSidebar")}>
        <SegmentedField
          label={t("sidebar.groupBy")}
          help={t("sidebar.groupByHelp")}
          value={settings.sessionGrouping}
          options={SESSION_GROUPINGS.map((grouping) => ({
            value: grouping,
            label: t(GROUPING_NAMES[grouping]),
          }))}
          onChange={(sessionGrouping) => {
            onChange({ sessionGrouping });
          }}
        />
        <TreeFlowField
          value={settings.treeFlow}
          supported={treeFlowSupported()}
          onChange={(treeFlow) => {
            onChange({ treeFlow });
          }}
        />
        <RowLayoutSetting
          value={settings.rowLayout}
          language={settings.language}
          onChange={(rowLayout) => {
            onChange({ rowLayout });
          }}
        />
      </SettingsGroup>
      <ViewerThemeGroup {...props} />
      <ResetLayoutLink onReset={props.onResetLayout} />
    </>
  );
}

/** "Reset layout" as a quiet link; what it does, then that it is done, next to it. */
function ResetLayoutLink({ onReset }: { onReset: () => void }) {
  const t = useT();
  const [done, setDone] = useState(false);
  return (
    <p className="setting-footer">
      <button
        type="button"
        className="link-button"
        aria-describedby="reset-layout-help"
        onClick={() => {
          onReset();
          setDone(true);
        }}
      >
        {t("settings.resetLayout")}
      </button>
      <span id="reset-layout-help" className="muted" role="status">
        {done ? t("settings.layoutResetDone") : t("settings.resetLayoutHelp")}
      </span>
    </p>
  );
}

/** Color scheme, font, size and cursor of the terminals, under a small live preview. */
function TerminalLookGroup(props: Props) {
  const { settings, onChange } = props;
  const t = useT();
  const fonts = fontChoices(availableFonts(), settings.terminalFont);
  return (
    <SettingsGroup title={t("settings.terminal")}>
      {/* A direct child of the card, so it can stay in view while the pickers scroll. */}
      <TerminalPreview settings={settings} theme={props.theme} fontSize={props.fontSize} />
      <WideRow label={t("settings.colorScheme")} help={t("settings.colorSchemeHelp")}>
        {({ labelId, helpId }) => (
          <div
            className="scheme-picker"
            role="radiogroup"
            aria-labelledby={labelId}
            aria-describedby={helpId}
          >
            {SCHEME_GROUPS.map((group) => (
              <div
                key={group}
                className="scheme-group"
                role="group"
                aria-labelledby={`scheme-group-${group}`}
              >
                <span id={`scheme-group-${group}`} className="scheme-group-title">
                  {t(SCHEME_GROUP_NAMES[group])}
                </span>
                <div className="scheme-grid">
                  {schemesInGroup(group).map((scheme) => (
                    <SchemeTile
                      key={scheme}
                      scheme={scheme}
                      name={t(SCHEME_NAMES[scheme])}
                      colors={schemeColors(scheme, props.theme)}
                      font={fontStack(settings.terminalFont)}
                      checked={settings.terminalScheme === scheme}
                      onSelect={() => {
                        onChange({ terminalScheme: scheme });
                      }}
                    />
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </WideRow>
      <WideRow label={t("settings.font")} help={t("settings.fontHelp")}>
        {({ labelId, helpId }) => (
          <div
            className="font-picker"
            role="radiogroup"
            aria-labelledby={labelId}
            aria-describedby={helpId}
          >
            {fonts.map((font) => (
              <label key={font} className="font-option" style={{ fontFamily: fontStack(font) }}>
                <input
                  type="radio"
                  name="terminal-font"
                  value={font}
                  checked={settings.terminalFont === font}
                  onChange={() => {
                    onChange({ terminalFont: font });
                  }}
                />
                <span className="font-option-name">
                  {font === "default" ? t("settings.fontDefault") : font}
                </span>
                <span className="font-option-sample" aria-hidden="true">
                  {FONT_SAMPLE}
                </span>
                <span className="font-option-check">
                  <CheckIcon size={14} />
                </span>
              </label>
            ))}
          </div>
        )}
      </WideRow>
      <div className="setting">
        <div className="setting-line">
          <RangeField
            id="font-size"
            label={t("settings.fontSize")}
            value={props.fontSize}
            min={FONT_SIZE.min}
            max={FONT_SIZE.max}
            display={t("settings.fontSizeValue", { size: props.fontSize })}
            onChange={(value) => {
              props.onFontSize(clampFontSize(value));
            }}
          />
        </div>
      </div>
      <CursorRow
        cursorStyle={settings.cursorStyle}
        blink={settings.cursorBlink}
        onStyle={(cursorStyle) => {
          onChange({ cursorStyle });
        }}
        onBlink={(cursorBlink) => {
          onChange({ cursorBlink });
        }}
      />
    </SettingsGroup>
  );
}

/** The cursor's form as segments and its blinking as a switch, side by side in one line. */
function CursorRow(props: {
  cursorStyle: CursorStyle;
  blink: boolean;
  onStyle: (style: CursorStyle) => void;
  onBlink: (blink: boolean) => void;
}) {
  const t = useT();
  const blinkId = `${useId()}-blink`;
  return (
    <SettingRow
      label={t("settings.cursor")}
      control={({ labelId }) => (
        <span className="cursor-control">
          <span className="segmented" role="radiogroup" aria-labelledby={labelId}>
            {CURSOR_STYLES.map((style) => (
              <label key={style} className="segment">
                <input
                  type="radio"
                  name="cursor-style"
                  value={style}
                  checked={props.cursorStyle === style}
                  onChange={() => {
                    props.onStyle(style);
                  }}
                />
                <span>{t(CURSOR_NAMES[style])}</span>
              </label>
            ))}
          </span>
          <label className="inline-switch">
            <Switch checked={props.blink} labelledBy={blinkId} onChange={props.onBlink} />
            <span id={blinkId}>{t("settings.cursorBlink")}</span>
          </label>
        </span>
      )}
    />
  );
}

/** Colors of the file viewer window: one tile per theme, each a few lines of code in it. */
function ViewerThemeGroup(props: Props) {
  const { settings, onChange } = props;
  const t = useT();
  return (
    <SettingsGroup title={t("settings.viewer")}>
      <WideRow label={t("settings.colorScheme")} help={t("settings.viewerHelp")}>
        {({ labelId, helpId }) => (
          <div
            className="viewer-theme-picker"
            role="radiogroup"
            aria-labelledby={labelId}
            aria-describedby={helpId}
          >
            {VIEWER_THEMES.map((theme) => {
              const palette = viewerPalette(theme, props.theme);
              return (
                <label
                  key={theme}
                  className="viewer-theme-option"
                  style={{ background: palette.background, color: palette.foreground }}
                >
                  <input
                    type="radio"
                    name="viewer-theme"
                    value={theme}
                    checked={settings.viewerTheme === theme}
                    onChange={() => {
                      onChange({ viewerTheme: theme });
                    }}
                  />
                  <CodeSample palette={palette} font={fontStack(settings.terminalFont)} />
                  <span
                    className="viewer-theme-name"
                    style={{ background: palette.activeLine, color: palette.foreground }}
                  >
                    {t(VIEWER_THEME_NAMES[theme])}
                  </span>
                  <span className="viewer-theme-check">
                    <CheckIcon size={14} />
                  </span>
                </label>
              );
            })}
          </div>
        )}
      </WideRow>
    </SettingsGroup>
  );
}

/** A few highlighted lines with line numbers in a viewer theme; decoration only. */
function CodeSample({ palette, font }: { palette: ViewerPalette; font: string }) {
  return (
    <span className="viewer-theme-code" aria-hidden="true" style={{ fontFamily: font }}>
      {PREVIEW_LINES.map((line, index) => (
        <span key={index} className="viewer-theme-line">
          <span className="viewer-theme-number" style={{ color: palette.gutterForeground }}>
            {index + 1}
          </span>
          {line.map(([kind, text], piece) => (
            <span
              key={piece}
              style={kind === null ? undefined : { color: palette.tokens[kind] }}
              className={kind === "comment" ? "viewer-theme-comment" : undefined}
            >
              {text}
            </span>
          ))}
        </span>
      ))}
    </span>
  );
}

/** One radio tile of the scheme picker: a short sample drawn in the scheme, and its name. */
function SchemeTile(props: {
  scheme: ColorScheme;
  name: string;
  colors: TerminalColors;
  font: string;
  checked: boolean;
  onSelect: () => void;
}) {
  const { colors } = props;
  return (
    <label
      className="scheme-option"
      style={{ background: colors.background, color: colors.foreground }}
    >
      <input
        type="radio"
        name="terminal-scheme"
        value={props.scheme}
        checked={props.checked}
        onChange={props.onSelect}
      />
      <span className="scheme-option-sample" aria-hidden="true" style={{ fontFamily: props.font }}>
        <PreviewText lines={TILE_LINES} colors={colors} />
      </span>
      <span className="scheme-option-palette" aria-hidden="true">
        {ACCENT_ROWS[0].map((color) => (
          <span key={color} style={{ background: colors[color] }} />
        ))}
      </span>
      <span
        className="scheme-option-name"
        style={{ background: `color-mix(in srgb, ${colors.foreground} 9%, ${colors.background})` }}
      >
        {props.name}
      </span>
      <span className="scheme-option-check">
        <CheckIcon size={14} />
      </span>
    </label>
  );
}

/** The 16 terminal colors as the usual two rows: normal, then bright. */
const ACCENT_ROWS: readonly [readonly AnsiColor[], readonly AnsiColor[]] = [
  ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"],
  [
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
  ],
];

/** Sample lines in a scheme's colors; each line a block, so the frame characters line up. */
function PreviewText({
  lines,
  colors,
  cursor,
}: {
  lines: readonly PreviewLine[];
  colors: TerminalColors;
  cursor?: ReactNode;
}) {
  return (
    <>
      {lines.map((line, index) => (
        <span key={index} className="preview-line">
          {line.map(([color, text], piece) => (
            <span key={piece} style={color === null ? undefined : { color: colors[color] }}>
              {text}
            </span>
          ))}
          {index === lines.length - 1 ? cursor : null}
        </span>
      ))}
    </>
  );
}

/**
 * A small terminal in the chosen scheme, font, size and cursor, with the scheme's 16 colors next
 * to it; decoration only. It stays in view while the pickers below it scroll.
 */
function TerminalPreview({
  settings,
  theme,
  fontSize,
}: {
  settings: Settings;
  theme: Theme;
  fontSize: number;
}) {
  const colors = schemeColors(settings.terminalScheme, theme);
  const cursor = (
    <span
      className={[
        "terminal-preview-cursor",
        settings.cursorStyle,
        settings.cursorBlink ? "blink" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      style={{ color: colors.cursor }}
    />
  );
  return (
    <div
      className="terminal-preview"
      aria-hidden="true"
      style={{ background: colors.background, color: colors.foreground }}
    >
      <span
        className="terminal-preview-text"
        style={{
          fontFamily: fontStack(settings.terminalFont),
          fontSize: `${String(Math.min(fontSize, PREVIEW_MAX_FONT_SIZE))}px`,
        }}
      >
        <PreviewText lines={TERMINAL_PREVIEW_LINES} colors={colors} cursor={cursor} />
      </span>
      <span className="terminal-preview-palette">
        {ACCENT_ROWS.map((row) =>
          row.map((color) => <span key={color} style={{ background: colors[color] }} />),
        )}
      </span>
    </div>
  );
}

/** Above this the sample frame no longer fits next to the palette in the dialog. */
const PREVIEW_MAX_FONT_SIZE = 15;

function ProgramsSection(props: Props) {
  return (
    <>
      <ProgramsSettings
        settings={props.settings}
        profiles={props.profiles}
        versions={props.versions}
        programUpdates={props.programUpdates}
        updateActions={props.updateActions}
        badgeFor={props.badgeFor}
        onChange={props.onChange}
        onProfiles={props.onProfiles}
      />
      <AgentInstructionsGroup
        settings={props.settings}
        profiles={props.profiles}
        onChange={props.onChange}
        onOpen={props.onOpenHangarMd}
      />
      <AvailableList profiles={availableProfiles(props.profiles)} />
      <ProgramsAdvanced
        settings={props.settings}
        onChange={props.onChange}
        onOpenProfileFile={props.onOpenProfileFile}
        onReloadProfiles={props.onReloadProfiles}
      />
    </>
  );
}

/** Hangar's own updates, then the installed programs' (npm, PyPI, GitHub). */
function UpdatesSection(props: Props) {
  const { settings, onChange, update, version, onCheckUpdates } = props;
  const t = useT();
  return (
    <>
      <SettingsGroup title={t("settings.groupHangar")}>
        <ActionRow
          label={
            version === null ? undefined : (
              <>
                {/* The placeholder comes last in every language (test in i18n.test.ts). */}
                {t("settings.installedVersion", { version: "" })}
                <VersionText version={version} />
              </>
            )
          }
          status={hangarUpdateStatus(update, props.updateCheckedAt, settings.language, t)}
          error={update.kind === "error"}
          action={t("settings.checkNow")}
          disabled={update.kind === "checking" || update.kind === "installing"}
          onAction={onCheckUpdates}
        />
        <SwitchRow
          label={t("settings.autoUpdate")}
          help={t("settings.autoUpdateHelp")}
          checked={settings.autoUpdate}
          onChange={(checked) => {
            onChange({ autoUpdate: checked });
          }}
        />
      </SettingsGroup>
      <ProgramUpdatesGroup settings={settings} state={props.programUpdates} onChange={onChange} />
    </>
  );
}

/** Switch for checking the programs for updates; "Check now" and its result while it is on. */
function ProgramUpdatesGroup({
  settings,
  state,
  onChange,
}: {
  settings: Settings;
  state: ProgramUpdateState;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  return (
    <SettingsGroup title={t("settings.groupPrograms")}>
      <SwitchRow
        label={t("settings.checkProgramUpdates")}
        help={t("settings.checkProgramUpdatesHelp")}
        checked={settings.checkProgramUpdates}
        onChange={(checked) => {
          onChange({ checkProgramUpdates: checked });
        }}
      >
        <ActionRow
          status={programUpdateStatus(state, settings.language, t)}
          error={state.error !== null}
          action={t("settings.checkNow")}
          disabled={state.checking}
          onAction={state.checkNow}
        />
      </SwitchRow>
    </SettingsGroup>
  );
}
