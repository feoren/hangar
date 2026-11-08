// Profile editor in Settings, Programs: the form model, splitting the argument line, checks before
// saving and the answers of `profile_env` and `profile_secrets`. The backend (profile_edit.rs)
// checks everything again; these checks only spare a round trip and point at the field.

import type { Profile, ProfileInput } from "./api";
import type { MessageKey } from "./i18n";

export interface EnvRow {
  /** Stable React key; never sent to the backend. */
  key: string;
  name: string;
  value: string;
  /**
   * The value is not known (a built-in entry's values never reach the frontend). Such a row is
   * saved only once the user types a value; otherwise "Customize" would write `NAME = ""` and
   * blank the variable for the program.
   */
  unknownValue?: boolean;
}

export interface SecretRow {
  key: string;
  name: string;
}

export interface ProfileForm {
  id: string;
  /** Not in profiles.toml yet ("New program" or "Customize" of a built-in entry). */
  isNew: boolean;
  name: string;
  command: string;
  /** All arguments as one line, split with `splitArgs`. */
  args: string;
  cwd: string;
  env: EnvRow[];
  secrets: SecretRow[];
}

let rowCounter = 0;

export function rowKey(): string {
  rowCounter += 1;
  return `row-${String(rowCounter)}`;
}

/** Same rule as the keychain account name and `secret_env` in the backend. */
const SECRET_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

export type SplitResult = { ok: true; args: string[] } | { ok: false };

/**
 * Splits an argument line like a shell would, without running one: spaces separate, `'...'` keeps
 * everything as typed, `"..."` keeps spaces and knows only `\"` and `\\`. Other backslashes stay,
 * so Windows paths can be typed as they are.
 */
export function splitArgs(line: string): SplitResult {
  const args: string[] = [];
  let current = "";
  let started = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < line.length; i += 1) {
    const char = line.charAt(i);
    if (quote === "'") {
      if (char === "'") quote = null;
      else current += char;
      continue;
    }
    if (quote === '"') {
      const next = line.charAt(i + 1);
      if (char === "\\" && (next === '"' || next === "\\")) {
        current += next;
        i += 1;
      } else if (char === '"') {
        quote = null;
      } else {
        current += char;
      }
      continue;
    }
    if (char === "'" || char === '"') {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started) args.push(current);
      current = "";
      started = false;
    } else {
      current += char;
      started = true;
    }
  }
  if (quote !== null) return { ok: false };
  if (started) args.push(current);
  return { ok: true, args };
}

/** The opposite of `splitArgs`: quotes only where needed, so `splitArgs(joinArgs(x))` is `x`. */
export function joinArgs(args: readonly string[]): string {
  return args
    .map((arg) => {
      if (arg !== "" && !/[\s'"]/.test(arg)) return arg;
      if (!arg.includes("'")) return `'${arg}'`;
      return `"${arg.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    })
    .join(" ");
}

/** Absolute on Windows (`C:\`, `\\server\share`) or POSIX (`/`); the backend refuses others. */
export function isAbsolutePath(path: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(path) || /^[\\/]{2}[^\\/]/.test(path) || path.startsWith("/");
}

/**
 * A plain variable whose name suggests a credential (API key, token, password). Only a hint:
 * the user decides.
 */
export function looksSecret(name: string): boolean {
  return /(^|_)(API)?KEYS?($|_)|APIKEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL/i.test(name);
}

/** Id for a new program: from its name, lowercase, unique without regard to case. */
export function newProfileId(name: string, taken: readonly string[]): string {
  const base =
    name
      .toLowerCase()
      .normalize("NFKD")
      .replace(/[\u0300-\u036f]/g, "")
      .replace(/[^a-z0-9._-]+/g, "-")
      .replace(/-+/g, "-")
      .replace(/^[-.]+|[-.]+$/g, "")
      .slice(0, 40) || "program";
  const used = new Set(taken.map((id) => id.toLowerCase()));
  if (!used.has(base)) return base;
  for (let number = 2; ; number += 1) {
    const id = `${base}-${String(number)}`;
    if (!used.has(id)) return id;
  }
}

export function emptyForm(id: string): ProfileForm {
  return { id, isNew: true, name: "", command: "", args: "", cwd: "", env: [], secrets: [] };
}

/**
 * Form for an existing entry. `envValues` comes from `profile_env` (empty for built-in entries,
 * whose values are never sent). A built-in entry opens as new: saving it creates the user's own
 * profile with the same id, which replaces the built-in one.
 */
export function formFromProfile(
  profile: Profile,
  envValues: Readonly<Record<string, string>>,
): ProfileForm {
  const names = profile.env ?? [];
  const extra = Object.keys(envValues).filter((name) => !names.includes(name));
  return {
    id: profile.id,
    isNew: profile.editable !== true,
    name: profile.name,
    command: profile.command,
    args: joinArgs(profile.args),
    cwd: profile.cwd ?? "",
    env: [...names, ...extra].map((name) => {
      const known = Object.hasOwn(envValues, name);
      return {
        key: rowKey(),
        name,
        value: known ? (envValues[name] ?? "") : "",
        ...(known || profile.editable === true ? {} : { unknownValue: true }),
      };
    }),
    secrets: (profile.secretEnv ?? []).map((name) => ({ key: rowKey(), name })),
  };
}

export type FormField = "name" | "command" | "args" | "cwd" | `env:${string}` | `secret:${string}`;

export interface FormProblem {
  field: FormField;
  message: MessageKey;
}

/** Rows the user left completely empty are not saved and not checked. */
function filledEnv(form: ProfileForm): EnvRow[] {
  return form.env.filter(
    (row) =>
      !(row.unknownValue === true && row.value === "") &&
      (row.name.trim() !== "" || row.value !== ""),
  );
}

function filledSecrets(form: ProfileForm): SecretRow[] {
  return form.secrets.filter((row) => row.name.trim() !== "");
}

/** Problems in field order; empty means the form can be saved. */
export function validateForm(form: ProfileForm): FormProblem[] {
  const problems: FormProblem[] = [];
  if (form.name.trim() === "") problems.push({ field: "name", message: "profile.nameMissing" });
  if (form.command.trim() === "") {
    problems.push({ field: "command", message: "profile.commandMissing" });
  }
  if (!splitArgs(form.args).ok) problems.push({ field: "args", message: "profile.argsQuote" });
  const cwd = form.cwd.trim();
  if (cwd !== "" && !isAbsolutePath(cwd)) {
    problems.push({ field: "cwd", message: "profile.cwdRelative" });
  }
  const secretNames = new Set(filledSecrets(form).map((row) => row.name.trim()));
  const envNames = new Set<string>();
  for (const row of filledEnv(form)) {
    const name = row.name.trim();
    const field: FormField = `env:${row.key}`;
    if (name === "" || name.includes("=") || name.includes("\0")) {
      problems.push({ field, message: "profile.envName" });
    } else if (envNames.has(name)) {
      problems.push({ field, message: "profile.duplicateName" });
    } else if (secretNames.has(name)) {
      problems.push({ field, message: "profile.alsoSecret" });
    } else if (row.value.includes("\0")) {
      problems.push({ field, message: "profile.envName" });
    }
    envNames.add(name);
  }
  const seen = new Set<string>();
  for (const row of filledSecrets(form)) {
    const name = row.name.trim();
    const field: FormField = `secret:${row.key}`;
    if (!SECRET_NAME.test(name)) problems.push({ field, message: "profile.secretName" });
    else if (seen.has(name)) problems.push({ field, message: "profile.duplicateName" });
    seen.add(name);
  }
  return problems;
}

/** What `save_profile` receives; call only after `validateForm` found nothing. */
export function toInput(form: ProfileForm): ProfileInput {
  const split = splitArgs(form.args);
  const cwd = form.cwd.trim();
  return {
    id: form.id,
    name: form.name.trim(),
    command: form.command.trim(),
    args: split.ok ? split.args : [],
    cwd: cwd === "" ? null : cwd,
    // Through a Map: `Object.fromEntries` defines own properties, so a variable named
    // `__proto__` or `constructor` stays an ordinary entry and never touches the prototype.
    env: Object.fromEntries(new Map(filledEnv(form).map((row) => [row.name.trim(), row.value]))),
    secretEnv: filledSecrets(form).map((row) => row.name.trim()),
  };
}

/** Answer of `profile_env`: name to value; anything that is not text is left out. */
export function parseProfileEnv(value: unknown): Record<string, string> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const env = new Map<string, string>();
  for (const [name, text] of Object.entries(value)) {
    if (typeof text === "string") env.set(name, text);
  }
  // Own properties only (no assignment), so `__proto__` cannot change the prototype.
  return Object.fromEntries(env);
}

/** Answer of `profile_secrets`: per name whether a value is stored. */
export function parseSecretStatuses(value: unknown): Map<string, boolean> {
  const statuses = new Map<string, boolean>();
  if (!Array.isArray(value)) return statuses;
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const name: unknown = Reflect.get(item, "name");
    const stored: unknown = Reflect.get(item, "stored");
    if (typeof name === "string" && typeof stored === "boolean") statuses.set(name, stored);
  }
  return statuses;
}
