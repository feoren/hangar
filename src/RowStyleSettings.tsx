// The row style in the settings (Appearance): one option per style and, below them, a live
// preview of a group with two sessions drawn in the chosen style, from the same markup and CSS as
// the sidebar's rows (SessionRow.tsx, styles.css "Row styles").

import type { SessionContext } from "./contextUsage";
import { badgeColor } from "./format";
import { useT, type MessageKey } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import { formatCpu, formatMemory, type ResourceParts } from "./resources";
import { ROW_STYLES, rowParts, stateCounts, type RowStyle } from "./rowStyle";
import { edgeClass, type ChipKind } from "./sessionChip";
import { RowColumnsHead, RowExpander, SessionRowBody } from "./SessionRow";
import { AgentGroupSection } from "./Sidebar";
import { lookClass, type LookState } from "./stateLook";
import { formatDuration } from "./statusLine";

export const ROW_STYLE_NAMES: Record<RowStyle, MessageKey> = {
  standard: "rowStyle.standard",
  pill: "rowStyle.pill",
  avatar: "rowStyle.avatar",
  accordion: "rowStyle.accordion",
  tags: "rowStyle.tags",
  ring: "rowStyle.ring",
  tiles: "rowStyle.tiles",
  edge: "rowStyle.edge",
  band: "rowStyle.band",
  segments: "rowStyle.segments",
  compact: "rowStyle.compact",
  counters: "rowStyle.counters",
  columns: "rowStyle.columns",
  card: "rowStyle.card",
};

export function RowStyleField({
  value,
  language,
  onChange,
}: {
  value: RowStyle;
  language: string;
  onChange: (value: RowStyle) => void;
}) {
  const t = useT();
  return (
    <div className="field">
      <span id="settings-row-style">{t("settings.rowStyle")}</span>
      <div
        className="row-style-options"
        role="radiogroup"
        aria-labelledby="settings-row-style"
        aria-describedby="settings-row-style-help"
      >
        {ROW_STYLES.map((style) => (
          <label key={style} className="segment row-style-option">
            <input
              type="radio"
              name="row-style"
              value={style}
              checked={value === style}
              onChange={() => {
                onChange(style);
              }}
            />
            <span>{t(ROW_STYLE_NAMES[style])}</span>
          </label>
        ))}
      </div>
      <RowStylePreview style={value} language={language} />
      <p id="settings-row-style-help" className="muted">
        {t("settings.rowStyleHelp")}
      </p>
    </div>
  );
}

interface PreviewRow {
  id: number;
  name: string;
  program: BadgeLook;
  look: LookState;
  chip: ChipKind;
  word: MessageKey;
  branch: string;
  usage: { cpu: number; memory: number };
  minutes: number;
  context: SessionContext;
}

const PREVIEW_ROWS: readonly PreviewRow[] = [
  {
    id: 1,
    name: "hangar",
    program: { kind: "letters", text: "C", color: badgeColor("claude") },
    look: "working",
    chip: "working",
    word: "status.short.working",
    branch: "feat/rows",
    usage: { cpu: 12.4, memory: 476 * 1024 * 1024 },
    minutes: 64,
    context: { used: 215_500, window: 1_000_000 },
  },
  {
    id: 2,
    name: "website",
    program: { kind: "letters", text: "Cx", color: badgeColor("codex") },
    look: "asks",
    chip: "asks",
    word: "status.short.asks",
    branch: "main",
    usage: { cpu: 0.3, memory: 182 * 1024 * 1024 },
    minutes: 14,
    context: { used: 168_000, window: 200_000 },
  },
];

/**
 * A group with a working session (on screen) and one with a question, as the sidebar would show
 * them in `style`. Decorative and inert: the options above are what the keyboard reaches.
 */
export function RowStylePreview({ style, language }: { style: RowStyle; language: string }) {
  const t = useT();
  const parts = rowParts(style);
  return (
    <div className="row-style-preview" data-rows={style} aria-hidden="true" inert>
      {parts.runtime === "columns" && <RowColumnsHead />}
      <ul className="session-list">
        <AgentGroupSection
          groupKey="preview"
          name={t("settings.preview")}
          look={null}
          count={PREVIEW_ROWS.length}
          open
          hasRows
          hiddenWaiting={0}
          holdsActive={false}
          counts={parts.counters ? stateCounts(PREVIEW_ROWS.map((row) => row.chip)) : []}
          onToggle={() => undefined}
          onMenu={() => undefined}
        >
          {PREVIEW_ROWS.map((row, index) => {
            const active = index === 0;
            const usage: ResourceParts = {
              cpu: formatCpu(row.usage.cpu, language),
              memory: formatMemory(row.usage.memory, language),
              gpu: null,
              gpuLoad: null,
              gpuMemory: null,
            };
            const word = t(row.word);
            return (
              <li
                key={row.id}
                className={[
                  "session",
                  active ? "active" : "",
                  row.look === "asks" ? "attention" : "",
                  lookClass(row.look),
                  edgeClass(row.look),
                  index === PREVIEW_ROWS.length - 1 ? "tree-last" : "",
                  parts.expander ? (active ? "expanded" : "folded") : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <span className="tree-guide" aria-hidden="true" />
                <span className="session-main">
                  <SessionRowBody
                    parts={parts}
                    name={row.name}
                    badge={null}
                    program={row.program}
                    marker={undefined}
                    line={{ word, full: word }}
                    waiting={row.look === "asks"}
                    chip={row.chip}
                    signs={null}
                    place={row.name}
                    branch={row.branch}
                    team={false}
                    usage={usage}
                    gpuColumn={false}
                    runtime={formatDuration(row.minutes * 60_000, language)}
                    context={row.context}
                    language={language}
                    moreId={`row-style-preview-${String(row.id)}`}
                  />
                </span>
                {parts.expander && (
                  <RowExpander
                    expanded={active}
                    controls={`row-style-preview-${String(row.id)}`}
                    onToggle={() => undefined}
                  />
                )}
              </li>
            );
          })}
        </AgentGroupSection>
      </ul>
    </div>
  );
}
