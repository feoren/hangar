import { useEffect, useRef, useState } from "react";

import type { Profile } from "./api";
import { Badge } from "./Badge";
import { BADGE_COLORS, badgeColor, badgeText, DEFAULT_BADGE_COLOR } from "./format";
import { agentDescriptionKey, useT, type MessageKey, type Translate } from "./i18n";
import { IconFileError, fileToIcon } from "./iconImage";
import { ArrowDownIcon, ArrowUpIcon, CopyIcon, GripIcon } from "./icons";
import {
  arrangedProfiles,
  availableProfiles,
  DEFAULT_PROGRAM_PREFS,
  iconChoice,
  iconMode,
  isHidden,
  moveTo,
  normalizeLetters,
  withHidden,
  withIcon,
  type BadgeLook,
  type IconMode,
  type ProgramPrefs,
} from "./programPrefs";
import type { ProgramVersions } from "./programs";
import type { ProgramUpdate } from "./programUpdates";
import type { Settings } from "./settings";
import { formatShortTime } from "./format";
import type { ProgramUpdateState } from "./useProgramUpdates";

interface Props {
  settings: Settings;
  profiles: Profile[];
  versions: ProgramVersions;
  programUpdates: ProgramUpdateState;
  /** Profile IDs with a running session (hint about locked files during an update). */
  runningPrograms: ReadonlySet<string>;
  /** Launches the update as a session; only offered when the backend reports `canRun`. */
  onRunUpdate: (profileId: string) => void;
  badgeFor: (profileId: string) => BadgeLook;
  onChange: (patch: Partial<Settings>) => void;
}

/** Color names of the letter palette, in the order of BADGE_COLORS. */
const BADGE_COLOR_NAMES: readonly MessageKey[] = [
  "color.blue",
  "color.violet",
  "color.teal",
  "color.orange",
  "color.pink",
  "color.green",
];

const ICON_MODES: readonly { mode: IconMode; label: MessageKey }[] = [
  { mode: "auto", label: "settings.iconAuto" },
  { mode: "letters", label: "settings.iconLetters" },
  { mode: "image", label: "settings.iconImage" },
];

type FocusTarget = { id: string; button: "up" | "down" };

/**
 * "Terminals & Agents" section: order installed entries (drag by the handle or arrow buttons),
 * show/hide them, pick an icon. Below that, catalog agents that are not installed yet, and the
 * switches for versions and program icons.
 */
export function ProgramsSettings(props: Props) {
  const { settings, profiles, versions, programUpdates, badgeFor, onChange } = props;
  const t = useT();
  const prefs = settings.programs;
  const arranged = arrangedProfiles(profiles, prefs);
  const shownIds = arranged.map((profile) => profile.id);
  const [announcement, setAnnouncement] = useState("");
  const [resetDone, setResetDone] = useState(false);
  const [openEditor, setOpenEditor] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; index: number } | null>(null);
  const rows = useRef(new Map<string, HTMLLIElement>());
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocus = useRef<FocusTarget | null>(null);
  const dragStart = useRef<{ mids: { id: string; mid: number }[] } | null>(null);

  const update = (next: ProgramPrefs) => {
    setResetDone(false);
    onChange({ programs: next });
  };

  // After moving via a button: focus the same button of the moved row, or the other one at the
  // edge. React moves the row in the DOM, and focus could otherwise get lost.
  useEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    pendingFocus.current = null;
    const same = buttons.current.get(`${target.id}:${target.button}`);
    const other = buttons.current.get(`${target.id}:${target.button === "up" ? "down" : "up"}`);
    (same && !same.disabled ? same : other)?.focus();
  });

  const announceMove = (id: string, index: number) => {
    const profile = arranged.find((p) => p.id === id);
    if (!profile) return;
    setAnnouncement(
      t("settings.moved", { name: profile.name, position: index + 1, count: arranged.length }),
    );
  };

  const moveBy = (id: string, delta: -1 | 1) => {
    const index = shownIds.indexOf(id);
    const target = index + delta;
    if (index < 0 || target < 0 || target >= shownIds.length) return;
    pendingFocus.current = { id, button: delta < 0 ? "up" : "down" };
    update({ ...prefs, order: moveTo(shownIds, prefs.order, id, target) });
    announceMove(id, target);
  };

  /** Drop position while dragging: number of other rows whose middle lies above the pointer. */
  const dropIndex = (id: string, clientY: number) => {
    const mids = dragStart.current?.mids ?? [];
    return mids.filter((row) => row.id !== id && row.mid < clientY).length;
  };

  const onHandleDown = (event: React.PointerEvent<HTMLSpanElement>, id: string) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragStart.current = {
      mids: shownIds.map((rowId) => {
        const rect = rows.current.get(rowId)?.getBoundingClientRect();
        return { id: rowId, mid: rect ? rect.top + rect.height / 2 : 0 };
      }),
    };
    setDrag({ id, index: shownIds.indexOf(id) });
  };

  const onHandleMove = (event: React.PointerEvent<HTMLSpanElement>) => {
    if (!drag) return;
    const index = dropIndex(drag.id, event.clientY);
    if (index !== drag.index) setDrag({ id: drag.id, index });
  };

  const endDrag = (commit: boolean) => {
    if (drag && commit && drag.index !== shownIds.indexOf(drag.id)) {
      update({ ...prefs, order: moveTo(shownIds, prefs.order, drag.id, drag.index) });
      announceMove(drag.id, drag.index);
    }
    dragStart.current = null;
    setDrag(null);
  };

  // While dragging, the row already sits at its target position; it is saved on release.
  const shown = drag
    ? moveTo(shownIds, [], drag.id, drag.index).flatMap((id) => arranged.filter((p) => p.id === id))
    : arranged;
  const available = availableProfiles(profiles);

  return (
    <>
      <section className="settings-section" aria-labelledby="programs-installed">
        <h4 id="programs-installed" className="settings-subtitle">
          {t("settings.installedPrograms")}
        </h4>
        {arranged.length === 0 ? (
          <p className="muted">{t("settings.noInstalled")}</p>
        ) : (
          <>
            <p id="programs-installed-help" className="muted settings-intro">
              {t("settings.installedHelp")}
            </p>
            {/* Column header for the switches; each switch carries the text itself too (aria-label). */}
            <div className="program-list-head" aria-hidden="true">
              <span className="program-list-head-label">{t("settings.showInSidebar")}</span>
            </div>
            <ul
              className={drag ? "program-list dragging-active" : "program-list"}
              aria-labelledby="programs-installed"
              aria-describedby="programs-installed-help"
            >
              {shown.map((profile, index) => {
                const hidden = isHidden(prefs, profile.id);
                const version = versions.get(profile.id);
                const programUpdate = programUpdates.updates.get(profile.id);
                const editorOpen = openEditor === profile.id;
                const editorId = `icon-editor-${profile.id}`;
                const className = [
                  "program-item",
                  drag?.id === profile.id ? "dragging" : "",
                  hidden ? "is-hidden" : "",
                ]
                  .filter(Boolean)
                  .join(" ");
                return (
                  <li
                    key={profile.id}
                    className={className}
                    ref={(element) => {
                      if (element) rows.current.set(profile.id, element);
                      else rows.current.delete(profile.id);
                    }}
                  >
                    <div className="program-row">
                      {/* Mouse only; by keyboard, the arrow buttons move the row. */}
                      <span
                        className="drag-handle"
                        aria-hidden="true"
                        title={t("settings.dragHandle", { name: profile.name })}
                        onPointerDown={(event) => {
                          onHandleDown(event, profile.id);
                        }}
                        onPointerMove={onHandleMove}
                        onPointerUp={() => {
                          endDrag(true);
                        }}
                        onPointerCancel={() => {
                          endDrag(false);
                        }}
                      >
                        <GripIcon />
                      </span>
                      <Badge look={badgeFor(profile.id)} />
                      <span className="program-name">
                        <span className="program-title-line">
                          <span className="program-title" title={profile.name}>
                            {profile.name}
                          </span>
                          {hidden && <span className="program-tag">{t("settings.hidden")}</span>}
                        </span>
                        {version && <span className="program-version">{version}</span>}
                      </span>
                      <span className="program-actions">
                        <button
                          type="button"
                          className="icon-button"
                          ref={(element) => {
                            const key = `${profile.id}:up`;
                            if (element) buttons.current.set(key, element);
                            else buttons.current.delete(key);
                          }}
                          aria-label={t("settings.moveUp", { name: profile.name })}
                          title={t("settings.moveUp", { name: profile.name })}
                          disabled={index === 0}
                          onClick={() => {
                            moveBy(profile.id, -1);
                          }}
                        >
                          <ArrowUpIcon />
                        </button>
                        <button
                          type="button"
                          className="icon-button"
                          ref={(element) => {
                            const key = `${profile.id}:down`;
                            if (element) buttons.current.set(key, element);
                            else buttons.current.delete(key);
                          }}
                          aria-label={t("settings.moveDown", { name: profile.name })}
                          title={t("settings.moveDown", { name: profile.name })}
                          disabled={index === shown.length - 1}
                          onClick={() => {
                            moveBy(profile.id, 1);
                          }}
                        >
                          <ArrowDownIcon />
                        </button>
                        <button
                          type="button"
                          className={editorOpen ? "chip-button open" : "chip-button"}
                          aria-expanded={editorOpen}
                          aria-controls={editorId}
                          aria-label={t("settings.iconFor", { name: profile.name })}
                          onClick={() => {
                            setOpenEditor(editorOpen ? null : profile.id);
                          }}
                        >
                          {t("settings.editIcon")}
                        </button>
                      </span>
                      <span className="program-visible">
                        <input
                          type="checkbox"
                          role="switch"
                          className="switch"
                          checked={!hidden}
                          aria-label={`${t("settings.showInSidebar")}: ${profile.name}`}
                          title={`${t("settings.showInSidebar")}: ${profile.name}`}
                          onChange={(event) => {
                            update(withHidden(prefs, profile.id, !event.currentTarget.checked));
                          }}
                        />
                      </span>
                    </div>
                    {programUpdate?.updateAvailable && (
                      <UpdateLine
                        profile={profile}
                        update={programUpdate}
                        running={props.runningPrograms.has(profile.id)}
                        onRun={props.onRunUpdate}
                      />
                    )}
                    {editorOpen && (
                      <IconEditor id={editorId} profile={profile} prefs={prefs} onChange={update} />
                    )}
                  </li>
                );
              })}
            </ul>
          </>
        )}
        <div className="sr-only" role="status">
          {announcement}
        </div>
        <div className="settings-row">
          <button
            type="button"
            className="secondary-button"
            aria-describedby="reset-programs-help"
            onClick={() => {
              onChange({ programs: DEFAULT_PROGRAM_PREFS });
              setOpenEditor(null);
              setResetDone(true);
            }}
          >
            {t("settings.resetPrograms")}
          </button>
          <span id="reset-programs-help" className="muted" role="status">
            {resetDone ? t("settings.programsResetDone") : t("settings.resetProgramsHelp")}
          </span>
        </div>
      </section>

      <ProgramUpdatesSection settings={settings} state={programUpdates} onChange={onChange} />

      <AvailableList profiles={available} />

      <section className="settings-section" aria-labelledby="programs-display">
        <h4 id="programs-display" className="settings-subtitle">
          {t("settings.display")}
        </h4>
        <label className="check">
          <input
            type="checkbox"
            checked={settings.showVersions}
            aria-describedby="show-versions-help"
            onChange={(event) => {
              onChange({ showVersions: event.currentTarget.checked });
            }}
          />
          <span>{t("settings.showVersions")}</span>
        </label>
        <p id="show-versions-help" className="muted settings-help">
          {t("settings.showVersionsHelp")}
        </p>
        <label className="check">
          <input
            type="checkbox"
            checked={settings.showIcons}
            aria-describedby="show-icons-help"
            onChange={(event) => {
              onChange({ showIcons: event.currentTarget.checked });
            }}
          />
          <span>{t("settings.showIcons")}</span>
        </label>
        <p id="show-icons-help" className="muted settings-help">
          {t("settings.showIconsHelp")}
        </p>
      </section>
    </>
  );
}

/**
 * "0.166.0 → 0.167.0", plus "Update" (launches the update command as a new session, only with
 * `canRun`) and the command to copy as a fallback.
 */
function UpdateLine({
  profile,
  update,
  running,
  onRun,
}: {
  profile: Profile;
  update: ProgramUpdate;
  running: boolean;
  onRun: (profileId: string) => void;
}) {
  const t = useT();
  const [feedback, setFeedback] = useState<boolean | null>(null);
  const command = update.updateHint;
  const copy = (text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setFeedback(true);
      })
      .catch(() => {
        setFeedback(false);
      });
  };
  return (
    <div className="program-update">
      <span className="program-update-versions">
        <span className="dot update" aria-hidden="true" />
        <span className="sr-only">
          {t("update.programAvailable", { version: update.latest ?? "" })}
        </span>
        <span aria-hidden="true">
          {update.installed ?? "?"} → {update.latest}
        </span>
      </span>
      {command && update.canRun && (
        <button
          type="button"
          className="primary-button"
          aria-label={`${t("update.run")}: ${profile.name}`}
          title={t("update.runsCommand", { command })}
          onClick={() => {
            onRun(profile.id);
          }}
        >
          {t("update.run")}
        </button>
      )}
      {command && (
        <button
          type="button"
          className="secondary-button copy-button"
          aria-label={`${t("settings.copyUpdate")}: ${profile.name}`}
          title={command}
          onClick={() => {
            copy(command);
          }}
        >
          <CopyIcon />
          {t("settings.copyUpdate")}
        </button>
      )}
      <span
        className={feedback === false ? "copy-feedback error-text selectable" : "copy-feedback"}
        role="status"
      >
        {feedback === null
          ? ""
          : feedback
            ? t("settings.copied")
            : t("settings.copyFailed", { command: command ?? "" })}
      </span>
      {update.canRun && running && (
        <p className="muted settings-help program-update-warning">
          {t("update.lockedHint", { name: profile.name })}
        </p>
      )}
    </div>
  );
}

/** Switch for checking for program updates, "Check now" and the status. */
function ProgramUpdatesSection({
  settings,
  state,
  onChange,
}: {
  settings: Settings;
  state: ProgramUpdateState;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const t = useT();
  const count = [...state.updates.values()].filter((entry) => entry.updateAvailable).length;
  const status = state.checking
    ? t("settings.programChecking")
    : state.error !== null
      ? t("settings.programCheckFailed", { error: state.error })
      : state.checkedAt === null
        ? ""
        : `${count === 0 ? t("settings.programUpdatesNone") : t("settings.programUpdatesSome", { count })} ${t("settings.programCheckedAt", { time: formatShortTime(new Date(state.checkedAt), settings.language) })}`;
  return (
    <section className="settings-section" aria-labelledby="programs-updates">
      <h4 id="programs-updates" className="settings-subtitle">
        {t("settings.programUpdates")}
      </h4>
      <label className="check">
        <input
          type="checkbox"
          checked={settings.checkProgramUpdates}
          aria-describedby="program-updates-help"
          onChange={(event) => {
            onChange({ checkProgramUpdates: event.currentTarget.checked });
          }}
        />
        <span>{t("settings.checkProgramUpdates")}</span>
      </label>
      <p id="program-updates-help" className="muted settings-help">
        {t("settings.checkProgramUpdatesHelp")}
      </p>
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button"
          disabled={!settings.checkProgramUpdates || state.checking}
          onClick={state.checkNow}
        >
          {t("settings.checkNow")}
        </button>
        <span className={state.error !== null ? "muted error-text" : "muted"} role="status">
          {status}
        </span>
      </div>
    </section>
  );
}

/** Automatic, letters or a custom image for an entry. */
function IconEditor({
  id,
  profile,
  prefs,
  onChange,
}: {
  id: string;
  profile: Profile;
  prefs: ProgramPrefs;
  onChange: (next: ProgramPrefs) => void;
}) {
  const t = useT();
  const stored = iconChoice(prefs, profile.id);
  // "Custom image" without an image is only saved after the file is picked; until then it is chosen only here.
  const [mode, setMode] = useState<IconMode>(iconMode(prefs, profile.id));
  const [letters, setLetters] = useState(
    stored?.kind === "letters" ? stored.text : badgeText(profile, profile.id),
  );
  const [imageError, setImageError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const color =
    stored?.kind === "letters"
      ? stored.color
      : (badgeColorInPalette(profile.id) ?? DEFAULT_BADGE_COLOR);

  const setLettersChoice = (text: string, nextColor: string) => {
    const clean = normalizeLetters(text);
    if (clean)
      onChange(withIcon(prefs, profile.id, { kind: "letters", text: clean, color: nextColor }));
  };

  const chooseMode = (next: IconMode) => {
    setMode(next);
    setImageError(null);
    if (next === "auto") onChange(withIcon(prefs, profile.id, null));
    if (next === "letters") setLettersChoice(letters, color);
  };

  const onFile = async (file: File | undefined) => {
    if (!file) return;
    try {
      const src = await fileToIcon(file);
      setImageError(null);
      onChange(withIcon(prefs, profile.id, { kind: "image", src }));
    } catch (error) {
      setImageError(imageErrorText(error, t));
    }
  };

  return (
    <div id={id} className="icon-editor">
      <div
        className="segmented"
        role="radiogroup"
        aria-label={t("settings.iconFor", { name: profile.name })}
      >
        {ICON_MODES.map(({ mode: option, label }) => (
          <label key={option} className="segment">
            <input
              type="radio"
              name={`icon-mode-${profile.id}`}
              value={option}
              checked={mode === option}
              onChange={() => {
                chooseMode(option);
              }}
            />
            <span>{t(label)}</span>
          </label>
        ))}
      </div>

      {mode === "auto" && <p className="muted">{t("settings.iconAutoHelp")}</p>}

      {mode === "letters" && (
        <div className="icon-letters">
          <label className="icon-letters-field">
            <span>{t("settings.letters")}</span>
            <input
              className="text-input letters-input"
              value={letters}
              maxLength={8}
              aria-describedby={`${id}-letters-help`}
              onChange={(event) => {
                const text = normalizeLetters(event.currentTarget.value);
                setLetters(text);
                setLettersChoice(text, color);
              }}
            />
          </label>
          <span id={`${id}-letters-help`} className="muted">
            {t("settings.lettersHelp")}
          </span>
          <fieldset className="settings-group">
            <legend>{t("settings.badgeColor")}</legend>
            <div className="swatches">
              {BADGE_COLORS.map((swatch, index) => {
                const name = t(BADGE_COLOR_NAMES[index] ?? "color.blue");
                return (
                  <label key={swatch} className="swatch" title={name}>
                    <input
                      type="radio"
                      name={`icon-color-${profile.id}`}
                      value={swatch}
                      aria-label={name}
                      checked={color === swatch}
                      onChange={() => {
                        setLettersChoice(letters, swatch);
                      }}
                    />
                    <span className="swatch-color letter-swatch" style={{ background: swatch }}>
                      {normalizeLetters(letters).slice(0, 1)}
                    </span>
                  </label>
                );
              })}
            </div>
          </fieldset>
        </div>
      )}

      {mode === "image" && (
        <div className="icon-image">
          <div className="settings-row">
            {stored?.kind === "image" && <Badge look={stored} />}
            <button
              type="button"
              className="secondary-button"
              aria-describedby={`${id}-image-help`}
              onClick={() => fileInput.current?.click()}
            >
              {t("settings.chooseImage")}
            </button>
            <input
              ref={fileInput}
              type="file"
              accept="image/png,image/svg+xml"
              className="sr-only"
              tabIndex={-1}
              aria-hidden="true"
              onChange={(event) => {
                const input = event.currentTarget;
                void onFile(input.files?.[0]).finally(() => {
                  // Picking the same file again should fire an event again.
                  input.value = "";
                });
              }}
            />
          </div>
          <p id={`${id}-image-help`} className="muted" role="status">
            {imageError ?? t("settings.imageHelp")}
          </p>
        </div>
      )}
    </div>
  );
}

/** Catalog agents that are not installed, with project page and install command. */
function AvailableList({ profiles }: { profiles: Profile[] }) {
  const t = useT();
  const [feedback, setFeedback] = useState<{ id: string; ok: boolean } | null>(null);

  const copy = (profile: Profile, command: string) => {
    navigator.clipboard
      .writeText(command)
      .then(() => {
        setFeedback({ id: profile.id, ok: true });
      })
      .catch(() => {
        setFeedback({ id: profile.id, ok: false });
      });
  };

  return (
    <section className="settings-section" aria-labelledby="programs-available">
      <h4 id="programs-available" className="settings-subtitle">
        {t("settings.available")}
      </h4>
      {profiles.length === 0 ? (
        <p className="muted">{t("settings.noAvailable")}</p>
      ) : (
        <>
          <p className="muted settings-intro">{t("settings.availableHelp")}</p>
          <ul className="available-list">
            {profiles.map((profile) => {
              const command = profile.installHint;
              const state = feedback?.id === profile.id ? feedback : null;
              const descriptionKey = agentDescriptionKey(profile.id);
              return (
                <li key={profile.id} className="available-item">
                  <div className="available-head">
                    <span className="program-title">{profile.name}</span>
                    {command && (
                      <button
                        type="button"
                        className="secondary-button copy-button"
                        onClick={() => {
                          copy(profile, command);
                        }}
                      >
                        <CopyIcon />
                        {t("settings.copyInstall")}
                      </button>
                    )}
                  </div>
                  {descriptionKey && (
                    <p className="muted available-description">{t(descriptionKey)}</p>
                  )}
                  {profile.homepage && (
                    <p className="available-line">
                      <span className="muted">{t("settings.homepage")}: </span>
                      <span className="selectable available-url">{profile.homepage}</span>
                    </p>
                  )}
                  {command ? (
                    <code className="selectable install-command">{command}</code>
                  ) : (
                    <p className="muted">{t("settings.noInstallHint")}</p>
                  )}
                  <p
                    className={
                      state && !state.ok ? "copy-feedback error-text selectable" : "copy-feedback"
                    }
                    role="status"
                  >
                    {state
                      ? state.ok
                        ? t("settings.copied")
                        : t("settings.copyFailed", { command: command ?? "" })
                      : ""}
                  </p>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}

function badgeColorInPalette(id: string): string | undefined {
  const color = badgeColor(id);
  return BADGE_COLORS.find((swatch) => swatch === color);
}

function imageErrorText(error: unknown, t: Translate): string {
  if (error instanceof IconFileError) {
    if (error.problem === "type") return t("settings.imageType");
    if (error.problem === "size") return t("settings.imageSize");
  }
  return t("settings.imageFailed");
}
