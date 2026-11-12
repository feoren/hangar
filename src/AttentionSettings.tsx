import { Badge } from "./Badge";
import { playChime } from "./chimeAudio";
import { DOT_ANIMATIONS, dotAnimationClass, type DotAnimation } from "./dotAnimation";
import { badgeColor } from "./format";
import { useT, type MessageKey } from "./i18n";
import { RangeField } from "./RangeField";
import { edgeClass } from "./sessionChip";
import {
  CHIME_VOLUME,
  clampChimeVolume,
  clampQuietSeconds,
  QUIET_SECONDS,
  type Settings,
} from "./settings";
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
import { StatusChip } from "./StatusChip";
import type { BadgeLook } from "./programPrefs";
import type { ChipKind } from "./sessionChip";
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

/** What the preview row says and shows, exactly like a session in the sidebar. */
const PREVIEWS: Record<LookState, { status: MessageKey; chip: ChipKind }> = {
  working: { status: "status.working", chip: "working" },
  done: { status: "status.attention", chip: "done" },
  asks: { status: "status.asks", chip: "asks" },
  idle: { status: "status.idle", chip: "paused" },
  failed: { status: "status.failed", chip: "failed" },
};

const PREVIEW_BADGE: BadgeLook = { kind: "letters", text: "C", color: badgeColor("claude") };

/** States whose look shows without attention: a resting session and an error. */
const LOOKS_WITHOUT_ATTENTION: readonly LookState[] = ["idle", "failed"];

/**
 * Settings section "Attention": one card per session state (color, live preview, pulse), then the
 * ways Hangar tells you about a waiting session.
 */
export function AttentionSettings({ settings, theme, onChange }: Props) {
  const t = useT();
  const enabled = settings.attentionEnabled;
  return (
    <>
      <label className="check">
        <input
          type="checkbox"
          checked={enabled}
          aria-describedby="attention-help"
          onChange={(event) => {
            onChange({ attentionEnabled: event.currentTarget.checked });
          }}
        />
        <span>{t("settings.attentionEnabled")}</span>
      </label>
      <p id="attention-help" className="muted settings-help">
        {t("settings.attentionHelp")}
      </p>

      <h3 className="settings-subhead">{t("look.heading")}</h3>
      <div className="look-cards">
        {LOOK_STATES.map((state) => (
          <LookCard
            key={state}
            state={state}
            settings={settings}
            theme={theme}
            // Without attention, no session works or waits; a resting or failed one still shows
            // its look.
            disabled={!enabled && !LOOKS_WITHOUT_ATTENTION.includes(state)}
            onChange={onChange}
          />
        ))}
      </div>

      <fieldset className="settings-group attention-notices" disabled={!enabled}>
        <legend className="settings-subhead">{t("settings.attentionNotices")}</legend>
        <RangeField
          id="quiet-seconds"
          label={t("settings.quietTime")}
          value={settings.quietSeconds}
          min={QUIET_SECONDS.min}
          max={QUIET_SECONDS.max}
          display={t("settings.seconds", { count: settings.quietSeconds })}
          onChange={(value) => {
            onChange({ quietSeconds: clampQuietSeconds(value) });
          }}
        />

        <CheckField
          id="taskbar-flash"
          checked={settings.taskbarFlash}
          label={t("settings.taskbarFlash")}
          help={t("settings.taskbarFlashHelp")}
          onChange={(taskbarFlash) => {
            onChange({ taskbarFlash });
          }}
        />
        <CheckField
          id="notify-desktop"
          checked={settings.notifyDesktop}
          label={t("settings.notifyDesktop")}
          help={t("settings.notifyDesktopHelp")}
          onChange={(notifyDesktop) => {
            onChange({ notifyDesktop });
          }}
        />
        <CheckField
          id="chime"
          checked={settings.chime}
          label={t("settings.chime")}
          help={t("settings.chimeHelp")}
          onChange={(chime) => {
            onChange({ chime });
          }}
        />
        <div className="chime-row">
          <RangeField
            id="chime-volume"
            label={t("settings.chimeVolume")}
            value={settings.chimeVolume}
            min={CHIME_VOLUME.min}
            max={CHIME_VOLUME.max}
            step={CHIME_VOLUME.step}
            disabled={!settings.chime}
            display={t("settings.percent", { count: settings.chimeVolume })}
            onChange={(value) => {
              onChange({ chimeVolume: clampChimeVolume(value) });
            }}
          />
          <button
            type="button"
            className="secondary-button"
            disabled={!settings.chime}
            onClick={() => {
              playChime("waiting", settings.chimeVolume);
            }}
          >
            {t("settings.chimePreview")}
          </button>
        </div>
      </fieldset>
    </>
  );
}

function CheckField({
  id,
  checked,
  label,
  help,
  onChange,
}: {
  id: string;
  checked: boolean;
  label: string;
  help: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <>
      <label className="check">
        <input
          type="checkbox"
          checked={checked}
          aria-describedby={`${id}-help`}
          onChange={(event) => {
            onChange(event.currentTarget.checked);
          }}
        />
        <span>{label}</span>
      </label>
      <p id={`${id}-help`} className="muted settings-help">
        {help}
      </p>
    </>
  );
}

function LookCard({
  state,
  settings,
  theme,
  disabled,
  onChange,
}: {
  state: LookState;
  settings: Settings;
  theme: Theme;
  disabled: boolean;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  const look = settings.looks[state];
  const name = t(STATE_NAMES[state]);
  const chosen = chosenSwatch(state, look);
  const color = resolvedColor(settings.looks, state, theme);
  const preview = PREVIEWS[state];
  // An unseen error waits like done and asks, so its preview breathes the same way.
  const waits = lookWaits(state) || state === "failed";
  const id = `look-${state}`;
  const seconds = t("settings.seconds", {
    count: formatPulseSeconds(look.seconds, settings.language),
  });
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
    <fieldset className="look-card" disabled={disabled} aria-labelledby={`${id}-title`}>
      <div className="look-card-head">
        <span id={`${id}-title`} className="look-card-title">
          {name}
        </span>
        <button
          type="button"
          className="link-button"
          disabled={isDefaultLook(state, look)}
          onClick={() => {
            onChange({ looks: withLook(settings.looks, state, DEFAULT_LOOKS[state]) });
          }}
        >
          {t("look.reset")}
        </button>
      </div>

      <div className="look-card-main">
        <div className="look-color">
          <div className="look-swatches" role="radiogroup" aria-label={t("settings.color")}>
            <label className="swatch" title={t("look.default")}>
              <input
                type="radio"
                name={`${id}-color`}
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
                  name={`${id}-color`}
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

        <div className="attention-preview look-preview" aria-hidden="true">
          <span
            className={[
              "session",
              waits ? "attention" : "",
              lookClass(state),
              edgeClass(state),
            ].join(" ")}
          >
            <span className="session-main">
              <Badge look={PREVIEW_BADGE} />
              <span className="session-text">
                <span className="session-title">{t("settings.preview")}</span>
                <span className="session-meta">
                  <span className="state-dot" aria-hidden="true" />
                  <span className={waits ? "session-status attention-text" : "session-status"}>
                    {t(preview.status)}
                  </span>
                </span>
              </span>
            </span>
            <StatusChip kind={preview.chip} />
          </span>
        </div>
      </div>

      <div className="look-pulse">
        <div className="look-pulse-strength">
          <span id={`${id}-pulse`}>{t("look.pulse")}</span>
          <div className="segmented" role="radiogroup" aria-labelledby={`${id}-pulse`}>
            {PULSE_STRENGTHS.map((pulse) => (
              <label key={pulse} className="segment">
                <input
                  type="radio"
                  name={`${id}-pulse`}
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
        <RangeField
          id={`${id}-seconds`}
          label={t("look.duration")}
          value={look.seconds}
          min={PULSE_SECONDS.min}
          max={PULSE_SECONDS.max}
          step={PULSE_SECONDS.step}
          disabled={look.pulse === "off"}
          display={seconds}
          onChange={(value) => {
            update({ seconds: clampPulseSeconds(value) });
          }}
        />
      </div>

      {state === "working" && (
        <DotChoice
          value={settings.dotAnimation}
          onChange={(dotAnimation) => {
            onChange({ dotAnimation });
          }}
        />
      )}
    </fieldset>
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
    <div className="look-dots">
      <span id="look-dots-label">{t("look.dots")}</span>
      <div className="segmented" role="radiogroup" aria-labelledby="look-dots-label">
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
