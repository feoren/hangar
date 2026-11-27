// The row builder in the settings (Appearance): a live preview of a group with two sessions on
// top, drawn from the same markup and CSS as the sidebar's rows (SessionRow.tsx, styles.css "Row
// layout"); below it the four lines, each with a left and a right part holding the elements as
// chips. A chip is dragged to another place (pointer events), or chosen and moved with the arrow
// buttons or arrow keys; its form is picked on the chip, its "x" hides it, and a hidden element
// comes back with its "+". The presets are the row styles of earlier versions. The model and every
// move live in rowLayout.ts.

import { useEffect, useMemo, useRef, useState } from "react";

import type { OutputRate, SessionContext } from "./contextUsage";
import { formatUsd } from "./costs";
import { badgeColor } from "./format";
import { useT, type MessageKey } from "./i18n";
import { ArrowUpIcon, CloseIcon, GripIcon, PlusIcon } from "./icons";
import type { BadgeLook } from "./programPrefs";
import { formatCpu, formatMemory, type ResourceParts } from "./resources";
import { DisclosureRow } from "./SettingsParts";
import {
  DEFAULT_ROW_LAYOUT,
  ELEMENT_FORMS,
  hasForms,
  hiddenElements,
  hideElement,
  isRowPreset,
  matchingPreset,
  placeElement,
  planRow,
  presetLayout,
  ROW_DENSITIES,
  ROW_FRAMES,
  ROW_PRESETS,
  ROW_SIDES,
  rowAttributes,
  sameLayout,
  setFormByName,
  showElement,
  stepElement,
  type RowDensity,
  type RowElement,
  type RowForms,
  type RowFrame,
  type RowLayout,
  type RowPlan,
  type RowPreset,
  type RowSide,
  type RowSpot,
  type RowStep,
} from "./rowLayout";
import { edgeClass, type ChipKind } from "./sessionChip";
import { SessionRowBody } from "./SessionRow";
import { AgentGroupSection } from "./Sidebar";
import { lookClass, type LookState } from "./stateLook";
import { formatDuration } from "./statusLine";

export const ROW_PRESET_NAMES: Record<RowPreset, MessageKey> = {
  hangar: "rowStyle.hangar",
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

export const ROW_ELEMENT_NAMES: Record<RowElement, MessageKey> = {
  title: "rowElement.title",
  state: "rowElement.state",
  program: "rowElement.program",
  signs: "rowElement.signs",
  folder: "rowElement.folder",
  branch: "rowElement.branch",
  load: "rowElement.load",
  runtime: "rowElement.runtime",
  context: "rowElement.context",
  tokens: "rowElement.tokens",
  cost: "rowElement.cost",
  speed: "rowElement.speed",
};

type FormName = RowForms[keyof RowForms];

const FORM_NAMES: Record<FormName, MessageKey> = {
  word: "rowForm.word",
  pill: "rowForm.pill",
  symbol: "rowForm.symbol",
  edge: "rowForm.edge",
  band: "rowForm.band",
  icon: "rowForm.icon",
  avatar: "rowForm.avatar",
  text: "rowForm.text",
  tag: "rowForm.tag",
  pills: "rowForm.pills",
  tiles: "rowForm.tiles",
  bar: "rowForm.bar",
  mini: "rowForm.mini",
  ring: "rowForm.ring",
  segments: "rowForm.segments",
  number: "rowForm.number",
};

const FRAME_NAMES: Record<RowFrame, MessageKey> = {
  flat: "rowLayout.frameFlat",
  card: "rowLayout.frameCard",
};

const DENSITY_NAMES: Record<RowDensity, MessageKey> = {
  normal: "rowLayout.densityNormal",
  compact: "rowLayout.densityCompact",
};

const SIDE_NAMES: Record<RowSide, MessageKey> = {
  left: "rowLayout.left",
  right: "rowLayout.right",
};

const STEP_NAMES: Record<RowStep, MessageKey> = {
  left: "rowLayout.moveLeft",
  right: "rowLayout.moveRight",
  up: "rowLayout.moveUp",
  down: "rowLayout.moveDown",
};

const STEPS: readonly RowStep[] = ["left", "right", "up", "down"];

/** Arrow keys on a chosen chip move it like the arrow buttons do. */
const KEY_STEPS: Readonly<Record<string, RowStep>> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

/** Pointer travel before a press on a chip becomes a drag. */
const DRAG_THRESHOLD_PX = 4;

/** Where a dragged chip would land: a place on a line, or among the hidden ones. */
type DropTarget = { kind: "spot"; spot: RowSpot } | { kind: "hidden" };

interface Drag {
  element: RowElement;
  pointerId: number;
  x: number;
  y: number;
  moved: boolean;
  target: DropTarget | null;
}

/**
 * The builder behind one line in the Sidebar group: most people never change the row, so it stays
 * closed and the line names the template in use.
 */
export function RowLayoutSetting({
  value,
  language,
  onChange,
}: {
  value: RowLayout;
  language: string;
  onChange: (value: RowLayout) => void;
}) {
  const t = useT();
  const preset = matchingPreset(value);
  return (
    <DisclosureRow
      label={t("settings.rowStyleEdit")}
      help={preset === null ? t("rowLayout.own") : t(ROW_PRESET_NAMES[preset])}
    >
      <RowLayoutField value={value} language={language} onChange={onChange} />
    </DisclosureRow>
  );
}

export function RowLayoutField({
  value,
  language,
  onChange,
}: {
  value: RowLayout;
  language: string;
  onChange: (value: RowLayout) => void;
}) {
  const t = useT();
  const plan = useMemo(() => planRow(value), [value]);
  const [selected, setSelected] = useState<RowElement | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const editor = useRef<HTMLDivElement>(null);
  /** The control to focus once a move has put the chip in its new place. */
  const refocus = useRef<{ element: RowElement; control: string } | null>(null);
  /** A press that ended a drag is no click. */
  const dragged = useRef(false);
  const preset = matchingPreset(value);
  const hidden = hiddenElements(value);
  const name = (element: RowElement) => t(ROW_ELEMENT_NAMES[element]);

  useEffect(() => {
    const wanted = refocus.current;
    if (wanted === null) return;
    refocus.current = null;
    editor.current
      ?.querySelector<HTMLElement>(
        `[data-chip="${wanted.element}"] [data-control="${wanted.control}"]`,
      )
      ?.focus();
  });

  const announce = (next: RowLayout, element: RowElement) => {
    const spot = next.lines.findIndex(
      (line) => line.left.includes(element) || line.right.includes(element),
    );
    if (spot < 0) {
      setAnnouncement(t("rowLayout.announceHidden", { element: name(element) }));
      return;
    }
    const side = next.lines[spot]?.left.includes(element) === true ? "left" : "right";
    setAnnouncement(
      t("rowLayout.announcePlaced", {
        element: name(element),
        line: spot + 1,
        side: t(SIDE_NAMES[side]),
      }),
    );
  };

  const change = (next: RowLayout, element: RowElement, control: string | null) => {
    if (sameLayout(next, value)) return;
    if (control !== null) refocus.current = { element, control };
    onChange(next);
    announce(next, element);
  };

  const step = (element: RowElement, direction: RowStep) => {
    change(stepElement(value, element, direction), element, `step-${direction}`);
  };

  /** The drop place under the pointer: the part and the chip it would stand before. */
  const targetAt = (x: number, y: number): DropTarget | null => {
    const under = document.elementFromPoint(x, y);
    if (!(under instanceof Element)) return null;
    if (under.closest("[data-hidden-pool]") !== null) return { kind: "hidden" };
    const slot = under.closest<HTMLElement>("[data-slot-line]");
    if (slot === null) return null;
    const line = Number(slot.dataset.slotLine);
    const side = ROW_SIDES.find((entry) => entry === slot.dataset.slotSide);
    if (!Number.isInteger(line) || side === undefined) return null;
    const chips = [...slot.querySelectorAll<HTMLElement>("[data-chip]")];
    let index = chips.length;
    for (const [position, chip] of chips.entries()) {
      const box = chip.getBoundingClientRect();
      if (y < box.top || (y <= box.bottom && x < box.left + box.width / 2)) {
        index = position;
        break;
      }
    }
    return { kind: "spot", spot: { line, side, index } };
  };

  const onPointerDown = (event: React.PointerEvent<HTMLElement>, element: RowElement) => {
    if (event.button !== 0) return;
    dragged.current = false;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({
      element,
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      moved: false,
      target: null,
    });
  };

  const onPointerMove = (event: React.PointerEvent<HTMLElement>) => {
    if (drag === null || drag.pointerId !== event.pointerId) return;
    const far = Math.hypot(event.clientX - drag.x, event.clientY - drag.y) >= DRAG_THRESHOLD_PX;
    if (!drag.moved && !far) return;
    const target = targetAt(event.clientX, event.clientY);
    if (drag.moved && sameTarget(target, drag.target)) return;
    setDrag({ ...drag, moved: true, target });
  };

  const onPointerUp = (event: React.PointerEvent<HTMLElement>) => {
    if (drag === null || drag.pointerId !== event.pointerId) return;
    setDrag(null);
    if (!drag.moved) return;
    // The click that follows this release is no choice; a later one is.
    dragged.current = true;
    window.setTimeout(() => {
      dragged.current = false;
    }, 0);
    const target = targetAt(event.clientX, event.clientY);
    if (target === null) return;
    const next =
      target.kind === "hidden"
        ? hideElement(value, drag.element)
        : placeElement(value, drag.element, target.spot);
    change(next, drag.element, null);
  };

  const dragProps = (element: RowElement) => ({
    onPointerDown: (event: React.PointerEvent<HTMLElement>) => {
      onPointerDown(event, element);
    },
    onPointerMove,
    onPointerUp,
    onPointerCancel: () => {
      setDrag(null);
    },
    onLostPointerCapture: () => {
      setDrag((current) => (current?.element === element ? null : current));
    },
  });

  const dropSpot = drag?.moved === true && drag.target?.kind === "spot" ? drag.target.spot : null;

  return (
    <div className="field row-builder">
      {/* Named by the line that opens it (RowLayoutSetting); the name stays for the editor. */}
      <span id="settings-row-layout" className="sr-only">
        {t("settings.rowStyle")}
      </span>
      <p id="settings-row-layout-help" className="muted">
        {t("settings.rowStyleHelp")}
      </p>
      <RowLayoutPreview plan={plan} language={language} />

      <div className="row-builder-tools">
        <label className="row-builder-preset">
          <span>{t("rowLayout.preset")}</span>
          <select
            className="select"
            value={preset ?? ""}
            onChange={(event) => {
              const chosen = event.currentTarget.value;
              if (isRowPreset(chosen)) {
                setSelected(null);
                onChange(presetLayout(chosen));
              }
            }}
          >
            {preset === null && <option value="">{t("rowLayout.own")}</option>}
            {ROW_PRESETS.map((entry) => (
              <option key={entry} value={entry}>
                {t(ROW_PRESET_NAMES[entry])}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="secondary-button"
          disabled={sameLayout(value, DEFAULT_ROW_LAYOUT)}
          onClick={() => {
            setSelected(null);
            onChange(DEFAULT_ROW_LAYOUT);
          }}
        >
          {t("rowLayout.reset")}
        </button>
      </div>

      <div
        ref={editor}
        className={drag?.moved === true ? "row-builder-editor dragging" : "row-builder-editor"}
        role="group"
        aria-labelledby="settings-row-layout"
        aria-describedby="settings-row-layout-help"
      >
        {value.lines.map((line, lineIndex) => (
          <div key={lineIndex} className="row-builder-line">
            <span className="row-builder-line-name" aria-hidden="true">
              {lineIndex + 1}
            </span>
            {ROW_SIDES.map((side) => {
              const elements = line[side];
              const label = t("rowLayout.slot", {
                line: lineIndex + 1,
                side: t(SIDE_NAMES[side]),
              });
              const active =
                dropSpot !== null && dropSpot.line === lineIndex && dropSpot.side === side;
              return (
                <div
                  key={side}
                  className={["row-builder-slot", `row-builder-slot-${side}`, active ? "drop" : ""]
                    .filter(Boolean)
                    .join(" ")}
                  role="list"
                  aria-label={label}
                  data-slot-line={lineIndex}
                  data-slot-side={side}
                >
                  {elements.map((element, index) => (
                    <RowChip
                      key={element}
                      element={element}
                      value={value}
                      selected={selected === element}
                      lifted={drag?.moved === true && drag.element === element}
                      insertBefore={active && dropSpot.index === index}
                      dragProps={dragProps(element)}
                      onSelect={() => {
                        if (dragged.current) {
                          dragged.current = false;
                          return;
                        }
                        setSelected((current) => (current === element ? null : element));
                      }}
                      onStep={(direction) => {
                        step(element, direction);
                      }}
                      onKeyStep={(direction) => {
                        change(stepElement(value, element, direction), element, "name");
                      }}
                      onForm={(form) => {
                        if (hasForms(element))
                          change(setFormByName(value, element, form), element, null);
                      }}
                      onHide={() => {
                        if (selected === element) setSelected(null);
                        // The focus moves on to its "+" among the hidden ones.
                        change(hideElement(value, element), element, "name");
                      }}
                    />
                  ))}
                  {active && dropSpot.index >= elements.length && (
                    <span className="row-builder-marker" aria-hidden="true" />
                  )}
                  {elements.length === 0 && !active && (
                    <span className="row-builder-empty" aria-hidden="true">
                      {t(side === "left" ? "rowLayout.emptyLeft" : "rowLayout.emptyRight")}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        ))}

        <div
          className={
            drag?.moved === true && drag.target?.kind === "hidden"
              ? "row-builder-hidden drop"
              : "row-builder-hidden"
          }
          data-hidden-pool=""
        >
          <span className="row-builder-hidden-name" id="row-builder-hidden">
            {t("rowLayout.hidden")}
          </span>
          {hidden.length === 0 ? (
            <span className="muted">{t("rowLayout.noneHidden")}</span>
          ) : (
            <ul className="row-builder-hidden-list" aria-labelledby="row-builder-hidden">
              {hidden.map((element) => (
                <li key={element} data-chip={element}>
                  <button
                    type="button"
                    className="row-chip row-chip-add"
                    data-control="name"
                    aria-label={t("rowLayout.show", { element: name(element) })}
                    {...dragProps(element)}
                    onClick={() => {
                      if (dragged.current) {
                        dragged.current = false;
                        return;
                      }
                      change(showElement(value, element), element, "name");
                    }}
                  >
                    <PlusIcon size={11} />
                    <span>{name(element)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>

      <div className="row-builder-options">
        <OptionGroup
          id="row-frame"
          label={t("rowLayout.frame")}
          options={ROW_FRAMES}
          names={FRAME_NAMES}
          value={value.frame}
          onChange={(frame) => {
            onChange({ ...value, frame });
          }}
        />
        <OptionGroup
          id="row-density"
          label={t("rowLayout.density")}
          options={ROW_DENSITIES}
          names={DENSITY_NAMES}
          value={value.density}
          onChange={(density) => {
            onChange({ ...value, density });
          }}
        />
      </div>
      <span className="sr-only" aria-live="polite">
        {announcement}
      </span>
    </div>
  );
}

function sameTarget(a: DropTarget | null, b: DropTarget | null): boolean {
  if (a === null || b === null) return a === b;
  if (a.kind === "hidden" || b.kind === "hidden") return a.kind === b.kind;
  return (
    a.spot.line === b.spot.line && a.spot.side === b.spot.side && a.spot.index === b.spot.index
  );
}

interface DragProps {
  onPointerDown: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerMove: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: () => void;
  onLostPointerCapture: () => void;
}

/**
 * An element on a line: its name (the drag handle; a click chooses it), its form where it has
 * more than one, and the "x" that hides it. A chosen chip shows its four arrow buttons.
 */
function RowChip({
  element,
  value,
  selected,
  lifted,
  insertBefore,
  dragProps,
  onSelect,
  onStep,
  onKeyStep,
  onForm,
  onHide,
}: {
  element: RowElement;
  value: RowLayout;
  selected: boolean;
  lifted: boolean;
  insertBefore: boolean;
  dragProps: DragProps;
  onSelect: () => void;
  onStep: (step: RowStep) => void;
  onKeyStep: (step: RowStep) => void;
  onForm: (form: string) => void;
  onHide: () => void;
}) {
  const t = useT();
  const name = t(ROW_ELEMENT_NAMES[element]);
  const forms: readonly FormName[] = hasForms(element) ? ELEMENT_FORMS[element] : [];
  const form = hasForms(element) ? value.forms[element] : null;
  return (
    <>
      {insertBefore && <span className="row-builder-marker" aria-hidden="true" />}
      <div
        role="listitem"
        className={["row-chip", selected ? "selected" : "", lifted ? "lifted" : ""]
          .filter(Boolean)
          .join(" ")}
        data-chip={element}
      >
        <button
          type="button"
          className="row-chip-name"
          data-control="name"
          aria-pressed={selected}
          title={t("rowLayout.chipHint")}
          {...dragProps}
          onClick={onSelect}
          onKeyDown={(event) => {
            const direction = KEY_STEPS[event.key];
            if (!selected || direction === undefined || event.altKey || event.ctrlKey) return;
            event.preventDefault();
            onKeyStep(direction);
          }}
        >
          <GripIcon size={11} />
          <span>{name}</span>
        </button>
        {form !== null && (
          <select
            className="row-chip-form"
            aria-label={t("rowLayout.form", { element: name })}
            value={form}
            onChange={(event) => {
              onForm(event.currentTarget.value);
            }}
          >
            {forms.map((entry) => (
              <option key={entry} value={entry}>
                {t(FORM_NAMES[entry])}
              </option>
            ))}
          </select>
        )}
        {selected && (
          <span className="row-chip-steps">
            {STEPS.map((direction) => {
              // At an edge the step changes nothing. The button stays focusable (it may hold the
              // focus right after the move that brought the chip there) and says it is off.
              const blocked = sameLayout(stepElement(value, element, direction), value);
              return (
                <button
                  key={direction}
                  type="button"
                  className={`row-chip-step step-${direction}`}
                  data-control={`step-${direction}`}
                  aria-label={t(STEP_NAMES[direction], { element: name })}
                  aria-disabled={blocked}
                  title={t(STEP_NAMES[direction], { element: name })}
                  onClick={() => {
                    if (!blocked) onStep(direction);
                  }}
                >
                  <ArrowUpIcon size={11} />
                </button>
              );
            })}
          </span>
        )}
        {element !== "title" && (
          <button
            type="button"
            className="row-chip-hide"
            aria-label={t("rowLayout.hide", { element: name })}
            title={t("rowLayout.hide", { element: name })}
            onClick={onHide}
          >
            <CloseIcon size={10} />
          </button>
        )}
      </div>
    </>
  );
}

function OptionGroup<T extends string>({
  id,
  label,
  options,
  names,
  value,
  onChange,
}: {
  id: string;
  label: string;
  options: readonly T[];
  names: Record<T, MessageKey>;
  value: T;
  onChange: (value: T) => void;
}) {
  const t = useT();
  return (
    <div className="row-builder-option">
      <span id={`settings-${id}`}>{label}</span>
      <div className="segmented" role="radiogroup" aria-labelledby={`settings-${id}`}>
        {options.map((option) => (
          <label key={option} className="segment">
            <input
              type="radio"
              name={id}
              value={option}
              checked={value === option}
              onChange={() => {
                onChange(option);
              }}
            />
            <span>{t(names[option])}</span>
          </label>
        ))}
      </div>
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
  /** Estimated dollars so far. */
  cost: number;
  /** Output tokens per second, live or the last response's. */
  speed: OutputRate | null;
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
    cost: 3.42,
    speed: { value: 84, live: true },
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
    cost: 0.38,
    speed: null,
  },
];

/**
 * A group with a working session (on screen) and one with a question, as the sidebar would show
 * them with `plan`. Decorative and inert: the editor below is what the keyboard reaches.
 */
export function RowLayoutPreview({ plan, language }: { plan: RowPlan; language: string }) {
  const t = useT();
  return (
    <div className="row-layout-preview" {...rowAttributes(plan)} aria-hidden="true" inert>
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
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <span className="row-pulse" aria-hidden="true" />
                <span className="tree-guide" aria-hidden="true" />
                <span className="session-main">
                  <SessionRowBody
                    plan={plan}
                    name={row.name}
                    // The preview's group is a project group, so the small icon shows too.
                    badge={row.program}
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
                    cost={`~${formatUsd(row.cost, language)}`}
                    speed={row.speed}
                    language={language}
                  />
                </span>
              </li>
            );
          })}
        </AgentGroupSection>
      </ul>
    </div>
  );
}
