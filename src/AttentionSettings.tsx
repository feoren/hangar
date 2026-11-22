import { useId, useState } from "react";

import { Badge } from "./Badge";
import { playChime } from "./chimeAudio";
import { DOT_ANIMATIONS, dotAnimationClass, type DotAnimation } from "./dotAnimation";
import { badgeColor } from "./format";
import { useT, type MessageKey } from "./i18n";
import { ChevronIcon } from "./icons";
import { PhonePushSettings } from "./PhonePushSettings";
import { formatClockTime, parseClockTime, type QuietHours } from "./quietHours";
import { RangeField } from "./RangeField";
import { edgeClass } from "./sessionChip";
import {
  CHIME_VOLUME,
  clampChimeVolume,
  clampQuietSeconds,
  QUIET_SECONDS,
  type Settings,
} from "./settings";
import { Advanced, SettingsGroup, SwitchRow } from "./SettingsParts";
import {
  chosenSwatch,
  clampPulseSeconds,
  DEFAULT_LOOKS,
  defaultColor,
  formatPulseSeconds,
  isDefaultLook,
  lookClass,
  LOOK_STATES,
  lookWaits,
  normalizeColor,
  presetsFor,
  PULSE_SECONDS,
  PULSE_STRENGTHS,
  resolvedColor,
  withLook,
  type LookPresetId,
  type LookState,
  type PulseStrength,
} from "./stateLook";
import { clampStallMinutes, STALL_MINUTES } from "./stall";
import { clampWaitMinutes, WAIT_MINUTES } from "./waitHint";
import { StatusChip } from "./StatusChip";
import type { BadgeLook } from "./programPrefs";
import type { Theme } from "./theme";

interface Props {
  settings: Settings;
  /** Appearance shown right now, for the default colors of working and pause. */
  theme: Theme;
  onChange: (patch: Partial<Settings>) => void;
}

const STATE_NAMES: Record<LookState, MessageKey> = {
  working: "look.working",
  done: "look.done",
  asks: "look.asks",
  idle: "look.idle",
  failed: "look.failed",
};

const DOT_NAMES: Record<DotAnimation, MessageKey> = {
  bounce: "look.dotsBounce",
  wave: "look.dotsWave",
  pulse: "look.dotsPulse",
  fade: "look.dotsFade",
};

const PULSE_NAMES: Record<PulseStrength, MessageKey> = {
  off: "look.pulseOff",
  subtle: "look.pulseSubtle",
  medium: "look.pulseMedium",
  strong: "look.pulseStrong",
};

const COLOR_NAMES: Record<LookPresetId, MessageKey> = {
  cream: "color.cream",
  amber: "color.amber",
  mint: "color.mint",
  sky: "color.sky",
  violet: "color.violet",
  pink: "color.pink",
};

/** What the preview row says, exactly like a session in the sidebar. */
const PREVIEWS: Record<LookState, MessageKey> = {
  working: "status.short.working",
  done: "status.short.attention",
  asks: "status.short.asks",
  idle: "status.short.idle",
  failed: "status.short.failed",
};

const PREVIEW_BADGE: BadgeLook = { kind: "letters", text: "C", color: badgeColor("claude") };

/** States whose look shows without highlighting: a resting session and an error. */
const LOOKS_WITHOUT_ATTENTION: readonly LookState[] = ["idle", "failed"];

/**
 * Settings section "Notifications" (id `attention`): the master switch, then how Hangar tells you
 * about a waiting session, quiet hours, the push to the phone, the look of each state and, under
 * "Advanced", the timings. The notices, the push and the timings depend on the master switch
 * (App.tsx checks `attentionEnabled` before every notice), so they show only while it is on. Quiet
 * hours and the looks of a resting or failed session apply without it (quiet hours also hold back
 * the budget warning, costs.ts), so they always show.
 */
export function AttentionSettings({ settings, theme, onChange }: Props) {
  const t = useT();
  const enabled = settings.attentionEnabled;
  const states = enabled
    ? LOOK_STATES
    : LOOK_STATES.filter((state) => LOOKS_WITHOUT_ATTENTION.includes(state));
  return (
    <>
      <SettingsGroup>
        <SwitchRow
          label={t("settings.attentionEnabled")}
          help={t("settings.attentionHelp")}
          checked={enabled}
          onChange={(attentionEnabled) => {
            onChange({ attentionEnabled });
          }}
        />
      </SettingsGroup>

      {enabled && (
        <SettingsGroup title={t("settings.groupWaiting")}>
          <SwitchRow
            label={t("settings.taskbarFlash")}
            help={t("settings.taskbarFlashHelp")}
            checked={settings.taskbarFlash}
            onChange={(taskbarFlash) => {
              onChange({ taskbarFlash });
            }}
          />
          <SwitchRow
            label={t("settings.notifyDesktop")}
            help={t("settings.notifyDesktopHelp")}
            checked={settings.notifyDesktop}
            onChange={(notifyDesktop) => {
              onChange({ notifyDesktop });
            }}
          />
          <SwitchRow
            label={t("settings.chime")}
            help={t("settings.chimeHelp")}
            checked={settings.chime}
            onChange={(chime) => {
              onChange({ chime });
            }}
          >
            <div className="setting">
              <div className="setting-line chime-line">
                <RangeField
                  id="chime-volume"
                  label={t("settings.chimeVolume")}
                  value={settings.chimeVolume}
                  min={CHIME_VOLUME.min}
                  max={CHIME_VOLUME.max}
                  step={CHIME_VOLUME.step}
                  display={t("settings.percent", { count: settings.chimeVolume })}
                  onChange={(value) => {
                    onChange({ chimeVolume: clampChimeVolume(value) });
                  }}
                />
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => {
                    playChime("waiting", settings.chimeVolume);
                  }}
                >
                  {t("settings.chimePreview")}
                </button>
              </div>
            </div>
          </SwitchRow>
        </SettingsGroup>
      )}

      {/* Quiet hours also hold back the budget warning, which does not depend on the master
          switch, so they stay available while it is off. */}
      <SettingsGroup title={t("settings.groupQuietHours")}>
        <QuietHoursRow
          value={settings.quietHours}
          onChange={(quietHours) => {
            onChange({ quietHours });
          }}
        />
      </SettingsGroup>

      {enabled && <PhonePushSettings />}

      <SettingsGroup title={t("look.heading")} description={t("look.help")}>
        <LookList states={states} settings={settings} theme={theme} onChange={onChange} />
      </SettingsGroup>

      {enabled && (
        <Advanced>
          <SettingsGroup>
            <SliderRow
              id="quiet-seconds"
              label={t("settings.quietTime")}
              help={t("settings.quietTimeHelp")}
              value={settings.quietSeconds}
              min={QUIET_SECONDS.min}
              max={QUIET_SECONDS.max}
              display={t("settings.seconds", { count: settings.quietSeconds })}
              onChange={(value) => {
                onChange({ quietSeconds: clampQuietSeconds(value) });
              }}
            />
            <SliderRow
              id="stall-minutes"
              label={t("settings.stallTime")}
              help={t("settings.stallHelp")}
              value={settings.stallMinutes}
              min={STALL_MINUTES.min}
              max={STALL_MINUTES.max}
              display={
                settings.stallMinutes === 0
                  ? t("settings.stallOff")
                  : t("settings.minutes", { count: settings.stallMinutes })
              }
              onChange={(value) => {
                onChange({ stallMinutes: clampStallMinutes(value) });
              }}
            />
            <SliderRow
              id="wait-minutes"
              label={t("settings.waitTime")}
              help={t("settings.waitHelp")}
              value={settings.waitMinutes}
              min={WAIT_MINUTES.min}
              max={WAIT_MINUTES.max}
              display={
                settings.waitMinutes === 0
                  ? t("settings.stallOff")
                  : t("settings.minutes", { count: settings.waitMinutes })
              }
              onChange={(value) => {
                onChange({ waitMinutes: clampWaitMinutes(value) });
              }}
            />
          </SettingsGroup>
        </Advanced>
      )}
    </>
  );
}

/** A slider on its own line with a help below it that describes the slider. */
function SliderRow({
  id,
  label,
  help,
  value,
  min,
  max,
  display,
  onChange,
}: {
  id: string;
  label: string;
  help: string;
  value: number;
  min: number;
  max: number;
  display: string;
  onChange: (value: number) => void;
}) {
  return (
    <div className="setting">
      <div className="setting-line">
        <span className="setting-text">
          <RangeField
            id={id}
            label={label}
            value={value}
            min={min}
            max={max}
            describedBy={`${id}-help`}
            display={display}
            onChange={onChange}
          />
          <span id={`${id}-help`} className="setting-help">
            {help}
          </span>
        </span>
      </div>
    </div>
  );
}

/**
 * Quiet hours: on or off, and the local times between which Hangar neither flashes, chimes,
 * notifies, pushes nor warns about the budget. The end may lie before the start; the window then runs over midnight.
 */
function QuietHoursRow({
  value,
  onChange,
}: {
  value: QuietHours;
  onChange: (value: QuietHours) => void;
}) {
  const t = useT();
  const time = (key: "from" | "to", label: string) => (
    <label className="quiet-hours-time">
      <span>{label}</span>
      <input
        type="time"
        className="text-input"
        value={formatClockTime(value[key])}
        onChange={(event) => {
          // A cleared field keeps the last time; the window never ends up half defined.
          const minutes = parseClockTime(event.currentTarget.value);
          if (minutes !== null) onChange({ ...value, [key]: minutes });
        }}
      />
    </label>
  );
  return (
    <SwitchRow
      label={t("settings.quietHours")}
      help={t("settings.quietHoursHelp")}
      checked={value.enabled}
      onChange={(enabled) => {
        onChange({ ...value, enabled });
      }}
    >
      <div className="setting">
        <div className="setting-line quiet-hours-line">
          {time("from", t("settings.quietFrom"))}
          {time("to", t("settings.quietTo"))}
          {value.from === value.to && (
            <span className="setting-help quiet-hours-empty" role="status">
              {t("settings.quietHoursEmpty")}
            </span>
          )}
        </div>
      </div>
    </SwitchRow>
  );
}

/**
 * One line per state with its color and a live preview; a click opens that state's editor below
 * it. One editor at a time keeps the list calm.
 */
function LookList({
  states,
  settings,
  theme,
  onChange,
}: {
  states: readonly LookState[];
  settings: Settings;
  theme: Theme;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const [open, setOpen] = useState<LookState | null>(null);
  return (
    <>
      {states.map((state) => (
        <LookRow
          key={state}
          state={state}
          settings={settings}
          theme={theme}
          open={open === state}
          onToggle={() => {
            setOpen((current) => (current === state ? null : state));
          }}
          onChange={onChange}
        />
      ))}
    </>
  );
}

function LookRow({
  state,
  settings,
  theme,
  open,
  onToggle,
  onChange,
}: {
  state: LookState;
  settings: Settings;
  theme: Theme;
  open: boolean;
  onToggle: () => void;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  const id = `look-${state}`;
  const name = t(STATE_NAMES[state]);
  const color = resolvedColor(settings.looks, state, theme);
  // An unseen error waits like done and asks, so its preview breathes the same way.
  const waits = lookWaits(state) || state === "failed";
  return (
    // `look-card` keeps the preview moving while power saving stills the sidebar (styles.css).
    <div className="setting look-card">
      <button
        type="button"
        id={`${id}-toggle`}
        className="setting-line look-row"
        aria-expanded={open}
        aria-controls={open ? `${id}-editor` : undefined}
        onClick={onToggle}
      >
        <span className="look-dot" style={{ background: color }} aria-hidden="true" />
        <span className="setting-label look-row-name">{name}</span>
        <span className="attention-preview look-preview" aria-hidden="true">
          <span
            className={[
              "session",
              waits ? "attention" : "",
              lookClass(state),
              edgeClass(state),
            ].join(" ")}
          >
            <span className="row-pulse" aria-hidden="true" />
            <span className="session-main">
              <Badge look={PREVIEW_BADGE} />
              <span className="session-text">
                {/* Like a session row's first line: the title, its state at the right edge. */}
                <span className="row-line session-title-row">
                  <span className="session-title">{t("settings.preview")}</span>
                  <span className="row-push" />
                  <span className="session-state">
                    <span className="state-dot" aria-hidden="true" />
                    <span className={waits ? "session-status attention-text" : "session-status"}>
                      {t(PREVIEWS[state])}
                    </span>
                  </span>
                </span>
              </span>
            </span>
          </span>
        </span>
        <span className={open ? "look-chevron open" : "look-chevron"} aria-hidden="true">
          <ChevronIcon />
        </span>
      </button>
      {open && (
        <div
          id={`${id}-editor`}
          className="setting-children look-editor"
          role="group"
          aria-labelledby={`${id}-toggle`}
        >
          <LookEditor state={state} settings={settings} theme={theme} onChange={onChange} />
        </div>
      )}
    </div>
  );
}

/** Color, pulse and (for working) the dots of one state; changes show at once in the preview. */
function LookEditor({
  state,
  settings,
  theme,
  onChange,
}: {
  state: LookState;
  settings: Settings;
  theme: Theme;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  const name = useId();
  const look = settings.looks[state];
  const chosen = chosenSwatch(state, look);
  const color = resolvedColor(settings.looks, state, theme);
  const update = (patch: Parameters<typeof withLook>[2]) => {
    onChange({ looks: withLook(settings.looks, state, patch) });
  };
  const chosenName =
    chosen === "default"
      ? t("look.default")
      : chosen === "custom"
        ? t("settings.customColor")
        : t(COLOR_NAMES[chosen]);

  return (
    <>
      <div className="look-field">
        <span id={`${name}-color`} className="look-field-label">
          {t("settings.color")}
        </span>
        <div className="look-color">
          <div className="swatches" role="radiogroup" aria-labelledby={`${name}-color`}>
            <label className="swatch" title={t("look.default")}>
              <input
                type="radio"
                name={`${name}-color`}
                aria-label={t("look.default")}
                checked={chosen === "default"}
                onChange={() => {
                  update({ color: null });
                }}
              />
              <span className="swatch-color" style={{ background: defaultColor(state, theme) }} />
            </label>
            {presetsFor(state).map((preset) => (
              <label key={preset.id} className="swatch" title={t(COLOR_NAMES[preset.id])}>
                <input
                  type="radio"
                  name={`${name}-color`}
                  aria-label={t(COLOR_NAMES[preset.id])}
                  checked={chosen === preset.id}
                  onChange={() => {
                    update({ color: preset.color });
                  }}
                />
                <span className="swatch-color" style={{ background: preset.color }} />
              </label>
            ))}
            <label
              className={chosen === "custom" ? "swatch custom selected" : "swatch custom"}
              title={t("settings.customColor")}
            >
              <input
                type="color"
                aria-label={t("settings.customColor")}
                value={color}
                onChange={(event) => {
                  const picked = normalizeColor(event.currentTarget.value);
                  if (picked) update({ color: picked });
                }}
              />
            </label>
          </div>
          <span className="look-color-name muted">{chosenName}</span>
        </div>
      </div>

      <div className="look-field">
        <span id={`${name}-pulse`} className="look-field-label">
          {t("look.pulse")}
        </span>
        <div className="segmented" role="radiogroup" aria-labelledby={`${name}-pulse`}>
          {PULSE_STRENGTHS.map((pulse) => (
            <label key={pulse} className="segment">
              <input
                type="radio"
                name={`${name}-pulse`}
                value={pulse}
                checked={look.pulse === pulse}
                onChange={() => {
                  update({ pulse });
                }}
              />
              <span>{t(PULSE_NAMES[pulse])}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Without pulsing there is no duration to set. */}
      {look.pulse !== "off" && (
        <div className="look-field look-duration">
          <RangeField
            id={`look-${state}-seconds`}
            label={t("look.duration")}
            value={look.seconds}
            min={PULSE_SECONDS.min}
            max={PULSE_SECONDS.max}
            step={PULSE_SECONDS.step}
            display={t("settings.seconds", {
              count: formatPulseSeconds(look.seconds, settings.language),
            })}
            onChange={(value) => {
              update({ seconds: clampPulseSeconds(value) });
            }}
          />
        </div>
      )}

      {state === "working" && (
        <DotChoice
          value={settings.dotAnimation}
          onChange={(dotAnimation) => {
            onChange({ dotAnimation });
          }}
        />
      )}

      {!isDefaultLook(state, look) && (
        <div className="look-field">
          <button
            type="button"
            className="link-button"
            onClick={() => {
              onChange({ looks: withLook(settings.looks, state, DEFAULT_LOOKS[state]) });
              // The link leaves with the change; the focus goes back to the state's line.
              requestAnimationFrame(() => {
                document.getElementById(`look-${state}-toggle`)?.focus();
              });
            }}
          >
            {t("look.reset")}
          </button>
        </div>
      )}
    </>
  );
}

/** How the working dots move; every option shows its own motion as a live preview. */
function DotChoice({
  value,
  onChange,
}: {
  value: DotAnimation;
  onChange: (value: DotAnimation) => void;
}) {
  const t = useT();
  return (
    <div className="look-field">
      <span id="look-dots-label" className="look-field-label">
        {t("look.dots")}
      </span>
      <div className="segmented look-dots" role="radiogroup" aria-labelledby="look-dots-label">
        {DOT_ANIMATIONS.map((style) => (
          <label key={style} className={`segment look-dots-option ${dotAnimationClass(style)}`}>
            <input
              type="radio"
              name="look-dots"
              value={style}
              checked={value === style}
              onChange={() => {
                onChange(style);
              }}
            />
            <StatusChip kind="working" />
            <span>{t(DOT_NAMES[style])}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
