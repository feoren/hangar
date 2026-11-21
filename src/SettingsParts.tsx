// Shared building blocks of the Settings dialog: one setting per line inside a framed group, a
// switch for on/off, a segmented choice, a button with its status, an inline question and a
// collapsed "Advanced" part. Sections build on these so every page reads and behaves the same.

import { useEffect, useId, useRef, useState, type ReactNode, type Ref } from "react";

import { useT } from "./i18n";
import { ChevronIcon } from "./icons";

/** Ids a row hands to its control, so the control is named and described by the row's texts. */
export interface RowIds {
  labelId: string;
  helpId: string | undefined;
}

/** On/off as a switch: a checkbox with the switch role, so it is announced as "on" or "off". */
export function Switch({
  checked,
  onChange,
  labelledBy,
  describedBy,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  labelledBy?: string | undefined;
  describedBy?: string | undefined;
  /** Accessible name when no visible text names the switch. */
  label?: string | undefined;
  disabled?: boolean;
}) {
  return (
    <input
      type="checkbox"
      role="switch"
      className="switch switch-large"
      checked={checked}
      disabled={disabled}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      aria-label={labelledBy === undefined ? label : undefined}
      onChange={(event) => {
        onChange(event.currentTarget.checked);
      }}
    />
  );
}

/**
 * One setting: its name and an optional one-line help on the left, the control on the right.
 * `clickable` makes the whole line a label, so a click anywhere toggles a switch. `children`
 * (settings that only matter while this one is on) show indented below while `expanded`.
 */
export function SettingRow({
  label,
  help,
  helpIsStatus = false,
  helpIsError = false,
  control,
  clickable = false,
  expanded = true,
  children,
}: {
  label: ReactNode;
  help?: ReactNode;
  /** The help line reports a result (a live region read out when it changes). */
  helpIsStatus?: boolean;
  helpIsError?: boolean;
  control?: (ids: RowIds) => ReactNode;
  clickable?: boolean;
  expanded?: boolean;
  children?: ReactNode;
}) {
  const id = useId();
  const labelId = `${id}-label`;
  const hasHelp = help !== undefined || helpIsStatus;
  const helpId = hasHelp ? `${id}-help` : undefined;
  const content = (
    <>
      <span className="setting-text">
        <span id={labelId} className="setting-label">
          {label}
        </span>
        {hasHelp && (
          <span
            id={helpId}
            className={helpIsError ? "setting-help error-text" : "setting-help"}
            role={helpIsStatus ? "status" : undefined}
          >
            {help}
          </span>
        )}
      </span>
      {control && <span className="setting-control">{control({ labelId, helpId })}</span>}
    </>
  );
  return (
    <div className="setting">
      {clickable ? (
        <label className="setting-line">{content}</label>
      ) : (
        <div className="setting-line">{content}</div>
      )}
      {expanded && children !== undefined && children !== null && children !== false && (
        <div className="setting-children">{children}</div>
      )}
    </div>
  );
}

/**
 * A setting whose control needs the whole width, such as a row of tiles or a slider: its name and
 * help on top, the control below them.
 */
export function WideRow({
  label,
  help,
  children,
}: {
  label: ReactNode;
  help?: ReactNode;
  children: (ids: RowIds) => ReactNode;
}) {
  const id = useId();
  const labelId = `${id}-label`;
  const helpId = help === undefined ? undefined : `${id}-help`;
  return (
    <div className="setting">
      <div className="setting-line setting-wide">
        <span className="setting-text">
          <span id={labelId} className="setting-label">
            {label}
          </span>
          {help !== undefined && (
            <span id={helpId} className="setting-help">
              {help}
            </span>
          )}
        </span>
        <div className="setting-wide-control">{children({ labelId, helpId })}</div>
      </div>
    </div>
  );
}

/**
 * A rarely changed setting behind one line: a click on it opens `children` below, a second click
 * closes them again. Closed when the dialog opens, so the page stays short; `defaultOpen` opens
 * it instead when its current value is not the usual one.
 */
export function DisclosureRow({
  label,
  help,
  defaultOpen = false,
  children,
}: {
  label: ReactNode;
  /** One line about the current state, shown while closed and open. */
  help?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const id = useId();
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="setting">
      <button
        type="button"
        className="setting-line disclosure-row"
        aria-expanded={open}
        aria-controls={open ? `${id}-body` : undefined}
        onClick={() => {
          setOpen((current) => !current);
        }}
      >
        <span className="setting-text">
          <span className="setting-label">{label}</span>
          {help !== undefined && <span className="setting-help">{help}</span>}
        </span>
        <span className={open ? "look-chevron open" : "look-chevron"} aria-hidden="true">
          <ChevronIcon />
        </span>
      </button>
      {open && (
        <div id={`${id}-body`} className="setting-disclosure-body">
          {children}
        </div>
      )}
    </div>
  );
}

/** A setting that is on or off; its `children` show only while it is on. */
export function SwitchRow({
  label,
  help,
  checked,
  onChange,
  disabled = false,
  children,
}: {
  label: ReactNode;
  help?: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  children?: ReactNode;
}) {
  return (
    <SettingRow
      label={label}
      help={help}
      clickable={!disabled}
      expanded={checked}
      control={({ labelId, helpId }) => (
        <Switch
          checked={checked}
          disabled={disabled}
          labelledBy={labelId}
          describedBy={helpId}
          onChange={onChange}
        />
      )}
    >
      {children}
    </SettingRow>
  );
}

/** A button with the result of its action next to it (read out when it changes). */
export function ActionRow({
  label,
  status,
  error = false,
  action,
  disabled = false,
  buttonRef,
  onAction,
}: {
  /** Name of the line; without it the status takes its place and describes the button. */
  label?: ReactNode;
  status: ReactNode;
  error?: boolean;
  action: string;
  disabled?: boolean;
  /** For giving the focus back to the button after a question in its place. */
  buttonRef?: Ref<HTMLButtonElement>;
  onAction: () => void;
}) {
  const id = useId();
  const statusId = `${id}-status`;
  const statusLine = (
    <span
      id={statusId}
      className={error ? "setting-help error-text" : "setting-help"}
      role="status"
    >
      {status}
    </span>
  );
  return (
    <div className="setting">
      <div className="setting-line">
        <span className="setting-text">
          {label !== undefined && (
            <span id={`${id}-label`} className="setting-label">
              {label}
            </span>
          )}
          {statusLine}
        </span>
        <span className="setting-control">
          <button
            ref={buttonRef}
            type="button"
            className="secondary-button"
            disabled={disabled}
            aria-describedby={label !== undefined ? `${id}-label` : statusId}
            onClick={onAction}
          >
            {action}
          </button>
        </span>
      </div>
    </div>
  );
}

/** One choice among a few, as a row of segments (a radio group). */
export function SegmentedField<T extends string>({
  label,
  help,
  value,
  options,
  onChange,
}: {
  label: ReactNode;
  help?: ReactNode;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  const name = useId();
  return (
    <SettingRow
      label={label}
      help={help}
      control={({ labelId, helpId }) => (
        <span
          className="segmented"
          role="radiogroup"
          aria-labelledby={labelId}
          aria-describedby={helpId}
        >
          {options.map((option) => (
            <label key={option.value} className="segment">
              <input
                type="radio"
                name={name}
                value={option.value}
                checked={value === option.value}
                onChange={() => {
                  onChange(option.value);
                }}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </span>
      )}
    />
  );
}

/**
 * A titled group of settings in a framed card, with an optional one-line description. Without a
 * title it is a plain card, for the settings a section opens with.
 */
export function SettingsGroup({
  title,
  description,
  children,
}: {
  title?: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  if (title === undefined) {
    return (
      <div className="setting-group">
        {description !== undefined && <p className="muted setting-group-desc">{description}</p>}
        <div className="setting-card">{children}</div>
      </div>
    );
  }
  return (
    <section className="setting-group" aria-labelledby={`${id}-title`}>
      <h4 id={`${id}-title`} className="setting-group-title">
        {title}
      </h4>
      {description !== undefined && <p className="muted setting-group-desc">{description}</p>}
      <div className="setting-card">{children}</div>
    </section>
  );
}

/**
 * Asks in place before an action that cannot be undone. The focus lands on "Cancel", so an
 * accidental Enter does nothing harmful.
 */
export function InlineConfirm({
  text,
  confirm,
  danger = true,
  onConfirm,
  onCancel,
}: {
  text: ReactNode;
  confirm: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const id = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
  }, []);
  return (
    <div className="confirm-box" role="group" aria-labelledby={`${id}-text`}>
      <p id={`${id}-text`}>{text}</p>
      <div className="settings-row">
        <button
          type="button"
          className={danger ? "primary-button danger" : "primary-button"}
          onClick={onConfirm}
        >
          {confirm}
        </button>
        <button ref={cancel} type="button" className="secondary-button" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/**
 * Rarely needed settings or reference, collapsed under "Advanced" (or `summary`) until opened.
 * `defaultOpen` opens it when the dialog shows, for a problem in there the user should see.
 */
export function Advanced({
  defaultOpen = false,
  summary,
  children,
}: {
  defaultOpen?: boolean;
  /** The line to open it by; "Advanced" when left out. */
  summary?: string;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <details className="setting-advanced" open={defaultOpen}>
      <summary>{summary ?? t("settings.advanced")}</summary>
      <div className="setting-advanced-body">{children}</div>
    </details>
  );
}
