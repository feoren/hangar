import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState } from "react";

import { api, errorText, type Profile } from "./api";
import { useT } from "./i18n";
import { CloseIcon, FolderIcon, PlusIcon } from "./icons";
import {
  emptyForm,
  formFromProfile,
  looksSecret,
  newProfileId,
  parseProfileEnv,
  parseSecretStatuses,
  rowKey,
  toInput,
  validateForm,
  type EnvRow,
  type FormField,
  type FormProblem,
  type ProfileForm,
} from "./profileForm";

interface Props {
  /** The entry to edit; `null` for "New program". */
  profile: Profile | null;
  /** Ids of every entry, so a new program gets a free one. */
  takenIds: readonly string[];
  /** The backend saved or deleted: apply its new list. */
  onProfiles: (profiles: Profile[]) => void;
  onClose: () => void;
}

type Status = { kind: "idle" } | { kind: "saved" } | { kind: "error"; message: string };

/**
 * Form for one program: name, command, arguments, working folder, plain and secret variables.
 * Saving goes to profiles.toml at once (`save_profile`); secret values go to the keychain only
 * after the profile is saved, because the backend accepts names of saved profiles only. Secret
 * values never live in React state: they stay in the password fields until saved, then the fields
 * are cleared.
 */
export function ProfileEditor({ profile, takenIds, onProfiles, onClose }: Props) {
  const t = useT();
  const id = useId();
  const [form, setForm] = useState<ProfileForm | null>(() =>
    profile === null
      ? emptyForm("")
      : profile.editable === true
        ? null
        : formFromProfile(profile, {}),
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [problems, setProblems] = useState<FormProblem[]>([]);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [saving, setSaving] = useState(false);
  const [stored, setStored] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const [secretErrors, setSecretErrors] = useState<ReadonlyMap<string, string>>(() => new Map());
  const [confirmDelete, setConfirmDelete] = useState(false);
  /** Password fields by row key; read on save, never copied into state. */
  const secretInputs = useRef(new Map<string, HTMLInputElement>());
  /** Values moved from a plain variable, put into the new password field once it exists. */
  const pendingSecrets = useRef(new Map<string, string>());
  const root = useRef<HTMLDivElement>(null);
  /** Set at once, unlike `saving`, so a second Enter before the next render saves nothing. */
  const busy = useRef(false);

  // Values of the plain variables and the secret state are read only when the form opens.
  useEffect(() => {
    if (profile?.editable !== true) return;
    let disposed = false;
    api
      .profileEnv(profile.id)
      .then((value) => {
        if (!disposed) setForm(formFromProfile(profile, parseProfileEnv(value)));
      })
      .catch((e: unknown) => {
        if (!disposed) setLoadError(errorText(e));
      });
    if ((profile.secretEnv ?? []).length > 0) {
      api
        .profileSecrets(profile.id)
        .then((value) => {
          if (!disposed) setStored(parseSecretStatuses(value));
        })
        .catch(() => undefined);
    }
    return () => {
      disposed = true;
    };
    // Opened once per entry; a new list after saving must not reset the form.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const ready = form !== null;
  useEffect(() => {
    if (ready) root.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [ready]);

  if (loadError !== null) {
    return (
      <div className="profile-editor" ref={root}>
        <p className="error-text selectable" role="alert">
          {t("profile.loadFailed", { error: loadError })}
        </p>
        <div className="profile-actions">
          <button type="button" className="secondary-button" onClick={onClose}>
            {t("common.close")}
          </button>
        </div>
      </div>
    );
  }
  if (form === null) {
    return (
      <div className="profile-editor" ref={root}>
        <p className="muted" role="status">
          {t("changes.loading")}
        </p>
      </div>
    );
  }

  const problemFor = (field: FormField) => problems.find((p) => p.field === field);
  const change = (patch: Partial<ProfileForm>) => {
    setForm((current) => (current === null ? current : { ...current, ...patch }));
    setStatus({ kind: "idle" });
  };
  const setEnv = (key: string, patch: Partial<EnvRow>) => {
    change({ env: form.env.map((row) => (row.key === key ? { ...row, ...patch } : row)) });
  };
  const fieldId = (field: string) => `${id}-${field}`;
  const describedBy = (field: FormField) =>
    problemFor(field) ? `${fieldId(field)}-problem` : undefined;

  const moveToSecrets = (row: EnvRow) => {
    const key = rowKey();
    if (row.value !== "") pendingSecrets.current.set(key, row.value);
    change({
      env: form.env.filter((other) => other.key !== row.key),
      secrets: [...form.secrets, { key, name: row.name.trim() }],
    });
  };

  const pickFolder = async () => {
    try {
      const picked = await open(
        form.cwd.trim() === "" ? { directory: true } : { directory: true, defaultPath: form.cwd },
      );
      if (typeof picked === "string") change({ cwd: picked });
    } catch (e) {
      setStatus({ kind: "error", message: errorText(e) });
    }
  };

  const save = async () => {
    if (busy.current) return;
    const found = validateForm(form);
    setProblems(found);
    if (found.length > 0) {
      setStatus({ kind: "idle" });
      const first = found[0];
      if (first) root.current?.querySelector<HTMLElement>(`[data-field="${first.field}"]`)?.focus();
      return;
    }
    const profileId = form.id === "" ? newProfileId(form.name, takenIds) : form.id;
    const input = toInput({ ...form, id: profileId });
    busy.current = true;
    setSaving(true);
    try {
      onProfiles(await api.saveProfile(input));
    } catch (e) {
      setStatus({ kind: "error", message: errorText(e) });
      setSaving(false);
      busy.current = false;
      return;
    }
    setForm((current) =>
      current === null ? current : { ...current, id: profileId, isNew: false },
    );
    // Secret values only now: the backend accepts names of the saved profile only.
    const errors = new Map<string, string>();
    for (const row of form.secrets) {
      const field = secretInputs.current.get(row.key);
      const name = row.name.trim();
      if (!field || field.value === "" || name === "") continue;
      try {
        await api.setProfileSecret(profileId, name, field.value);
        field.value = "";
      } catch (e) {
        errors.set(row.key, errorText(e));
      }
    }
    setSecretErrors(errors);
    if (input.secretEnv.length > 0) {
      try {
        setStored(parseSecretStatuses(await api.profileSecrets(profileId)));
      } catch {
        // The fields keep their last known state.
      }
    }
    setSaving(false);
    busy.current = false;
    setStatus(errors.size === 0 ? { kind: "saved" } : { kind: "idle" });
  };

  const forgetSecret = async (name: string) => {
    try {
      await api.deleteProfileSecret(form.id, name);
      setStored((current) => new Map(current).set(name, false));
    } catch (e) {
      setStatus({ kind: "error", message: errorText(e) });
    }
  };

  const remove = async () => {
    try {
      onProfiles(await api.deleteProfile(form.id));
      onClose();
    } catch (e) {
      setConfirmDelete(false);
      setStatus({ kind: "error", message: errorText(e) });
    }
  };

  const problemText = (field: FormField) => {
    const problem = problemFor(field);
    return problem ? (
      <span id={`${fieldId(field)}-problem`} className="profile-problem error-text">
        {t(problem.message)}
      </span>
    ) : null;
  };
  const savedProfile = !form.isNew;

  return (
    <div
      className="profile-editor"
      ref={root}
      onKeyDown={(event) => {
        // Not while an input method composes (Japanese, Chinese): there Enter confirms the text.
        if (
          event.key === "Enter" &&
          !event.nativeEvent.isComposing &&
          event.target instanceof HTMLInputElement
        ) {
          event.preventDefault();
          void save();
        }
      }}
    >
      {profile !== null && profile.editable !== true && (
        <p className="muted profile-intro">{t("profile.customizeHelp")}</p>
      )}
      <div className="profile-grid">
        <label htmlFor={fieldId("name")}>{t("profile.name")}</label>
        <div className="profile-field">
          <input
            id={fieldId("name")}
            data-field="name"
            className="text-input"
            value={form.name}
            spellCheck={false}
            aria-invalid={problemFor("name") ? true : undefined}
            aria-describedby={describedBy("name")}
            onChange={(event) => {
              change({ name: event.currentTarget.value });
            }}
          />
          {problemText("name")}
        </div>

        <label htmlFor={fieldId("command")}>{t("profile.command")}</label>
        <div className="profile-field">
          <input
            id={fieldId("command")}
            data-field="command"
            className="text-input mono"
            value={form.command}
            spellCheck={false}
            placeholder={t("profile.commandPlaceholder")}
            aria-invalid={problemFor("command") ? true : undefined}
            aria-describedby={describedBy("command")}
            onChange={(event) => {
              change({ command: event.currentTarget.value });
            }}
          />
          {problemText("command")}
        </div>

        <label htmlFor={fieldId("args")}>{t("profile.args")}</label>
        <div className="profile-field">
          <input
            id={fieldId("args")}
            data-field="args"
            className="text-input mono"
            value={form.args}
            spellCheck={false}
            aria-invalid={problemFor("args") ? true : undefined}
            aria-describedby={
              problemFor("args") ? `${fieldId("args")}-problem` : `${fieldId("args")}-help`
            }
            onChange={(event) => {
              change({ args: event.currentTarget.value });
            }}
          />
          {problemText("args") ?? (
            <span id={`${fieldId("args")}-help`} className="muted profile-help">
              {t("profile.argsHelp")}
            </span>
          )}
        </div>

        <label htmlFor={fieldId("cwd")}>{t("profile.cwd")}</label>
        <div className="profile-field">
          <div className="profile-inline">
            <input
              id={fieldId("cwd")}
              data-field="cwd"
              className="text-input"
              value={form.cwd}
              spellCheck={false}
              placeholder={t("profile.cwdPlaceholder")}
              aria-invalid={problemFor("cwd") ? true : undefined}
              aria-describedby={describedBy("cwd")}
              onChange={(event) => {
                change({ cwd: event.currentTarget.value });
              }}
            />
            <button
              type="button"
              className="icon-button"
              aria-label={t("profile.pickCwd")}
              title={t("profile.pickCwd")}
              onClick={() => void pickFolder()}
            >
              <FolderIcon />
            </button>
          </div>
          {problemText("cwd")}
        </div>
      </div>

      <fieldset className="profile-vars">
        <legend>{t("profile.env")}</legend>
        {form.env.length === 0 && <p className="muted profile-help">{t("profile.envNone")}</p>}
        {form.env.map((row) => {
          const field: FormField = `env:${row.key}`;
          const secretHint = looksSecret(row.name);
          return (
            <div key={row.key} className="profile-var">
              <div className="profile-inline">
                <input
                  data-field={field}
                  className="text-input mono profile-var-name"
                  value={row.name}
                  spellCheck={false}
                  placeholder={t("profile.varName")}
                  aria-label={t("profile.varName")}
                  aria-invalid={problemFor(field) ? true : undefined}
                  aria-describedby={describedBy(field)}
                  onChange={(event) => {
                    setEnv(row.key, { name: event.currentTarget.value });
                  }}
                />
                <input
                  className="text-input mono profile-var-value"
                  value={row.value}
                  spellCheck={false}
                  placeholder={t("profile.varValue")}
                  aria-label={t("profile.varValueFor", { name: row.name || "…" })}
                  onChange={(event) => {
                    setEnv(row.key, { value: event.currentTarget.value });
                  }}
                />
                <button
                  type="button"
                  className="icon-button"
                  aria-label={t("profile.removeVar", { name: row.name || "…" })}
                  title={t("profile.removeVar", { name: row.name || "…" })}
                  onClick={() => {
                    change({ env: form.env.filter((other) => other.key !== row.key) });
                  }}
                >
                  <CloseIcon />
                </button>
              </div>
              {problemText(field)}
              {secretHint && !problemFor(field) && (
                <p className="profile-secret-hint">
                  <span>{t("profile.looksSecret")}</span>
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => {
                      moveToSecrets(row);
                    }}
                  >
                    {t("profile.makeSecret")}
                  </button>
                </p>
              )}
            </div>
          );
        })}
        <button
          type="button"
          className="link-button profile-add"
          onClick={() => {
            change({ env: [...form.env, { key: rowKey(), name: "", value: "" }] });
          }}
        >
          <PlusIcon size={12} />
          {t("profile.addVar")}
        </button>
      </fieldset>

      <fieldset className="profile-vars">
        <legend>{t("profile.secrets")}</legend>
        <p className="muted profile-help">{t("profile.secretsHelp")}</p>
        {form.secrets.map((row) => {
          const field: FormField = `secret:${row.key}`;
          const name = row.name.trim();
          const isStored = savedProfile && stored.get(name) === true;
          const error = secretErrors.get(row.key);
          return (
            <div key={row.key} className="profile-var">
              <div className="profile-inline">
                <input
                  data-field={field}
                  className="text-input mono profile-var-name"
                  value={row.name}
                  spellCheck={false}
                  placeholder={t("profile.varName")}
                  aria-label={t("profile.varName")}
                  aria-invalid={problemFor(field) ? true : undefined}
                  aria-describedby={describedBy(field)}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    change({
                      secrets: form.secrets.map((other) =>
                        other.key === row.key ? { ...other, name: value } : other,
                      ),
                    });
                  }}
                />
                <input
                  ref={(element) => {
                    if (element) {
                      secretInputs.current.set(row.key, element);
                      const pending = pendingSecrets.current.get(row.key);
                      if (pending !== undefined) {
                        element.value = pending;
                        pendingSecrets.current.delete(row.key);
                      }
                    } else {
                      secretInputs.current.delete(row.key);
                    }
                  }}
                  type="password"
                  autoComplete="new-password"
                  className="text-input mono profile-var-value"
                  placeholder={isStored ? t("profile.secretReplace") : t("profile.secretValue")}
                  aria-label={t("profile.secretValueFor", { name: name || "…" })}
                  aria-describedby={`${fieldId(field)}-state`}
                  onChange={() => {
                    setStatus({ kind: "idle" });
                  }}
                />
                <button
                  type="button"
                  className="icon-button"
                  aria-label={t("profile.removeVar", { name: name || "…" })}
                  title={t("profile.removeVar", { name: name || "…" })}
                  onClick={() => {
                    secretInputs.current.delete(row.key);
                    change({ secrets: form.secrets.filter((other) => other.key !== row.key) });
                  }}
                >
                  <CloseIcon />
                </button>
              </div>
              {problemText(field)}
              <p id={`${fieldId(field)}-state`} className="profile-secret-state">
                <span className={isStored ? "secret-state stored" : "secret-state"}>
                  <span className="secret-mark" aria-hidden="true" />
                  {isStored ? t("profile.secretStored") : t("profile.secretNotStored")}
                </span>
                {isStored && (
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => void forgetSecret(name)}
                  >
                    {t("profile.secretForget")}
                  </button>
                )}
              </p>
              {error !== undefined && (
                <p className="error-text selectable profile-problem" role="alert">
                  {error}
                </p>
              )}
            </div>
          );
        })}
        <button
          type="button"
          className="link-button profile-add"
          onClick={() => {
            change({ secrets: [...form.secrets, { key: rowKey(), name: "" }] });
          }}
        >
          <PlusIcon size={12} />
          {t("profile.addSecret")}
        </button>
      </fieldset>

      {status.kind === "error" && (
        <p className="error-text selectable" role="alert">
          {status.message}
        </p>
      )}
      <div className="profile-actions">
        {profile?.editable === true && savedProfile && (
          <span className="profile-delete">
            {confirmDelete ? (
              <>
                <span>{t("profile.deleteConfirm", { name: form.name || profile.name })}</span>
                <button
                  type="button"
                  className="primary-button danger"
                  onClick={() => void remove()}
                >
                  {t("profile.delete")}
                </button>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setConfirmDelete(false);
                  }}
                >
                  {t("common.cancel")}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="link-button danger-link"
                onClick={() => {
                  setConfirmDelete(true);
                }}
              >
                {t("profile.delete")}
              </button>
            )}
          </span>
        )}
        <span className="profile-saved muted" role="status">
          {status.kind === "saved" ? t("profile.saved") : ""}
        </span>
        <button type="button" className="secondary-button" onClick={onClose}>
          {status.kind === "saved" ? t("common.close") : t("common.cancel")}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={saving}
          onClick={() => void save()}
        >
          {t("profile.save")}
        </button>
      </div>
    </div>
  );
}
