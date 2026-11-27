// Models per start and the local model servers (ADR 0022): which models a program offers, whether
// Ollama and LM Studio run on this computer and which models they hold, what the top bar says
// about them and which sections the start menu shows. Pure: the backend probes the servers
// (`local_llm.rs`), the window only reads its answer (useLocalLlm.ts).

import type {
  LocalServerId,
  ModelSource,
  Profile,
  ProfileModel,
  SessionInfo,
  SessionModel,
} from "./api";
import type { Translate } from "./i18n";

/** The servers in the order the top bar and the start menu show them. */
export const LOCAL_SERVERS: readonly LocalServerId[] = ["ollama", "lmstudio"];

/** Product names; not translated. */
export const SERVER_NAMES: Readonly<Record<LocalServerId, string>> = {
  ollama: "Ollama",
  lmstudio: "LM Studio",
};

export interface LocalModel {
  id: string;
  label: string;
  /** In memory right now, so it answers without a wait. */
  loaded: boolean;
}

export interface LocalServer {
  running: boolean;
  models: LocalModel[];
}

export type LocalLlmState = Readonly<Record<LocalServerId, LocalServer>>;

/** As the backend bounds them (ADR 0022). */
const MAX_MODELS = 200;
const MAX_ID_CHARS = 160;
const MODEL_ID = /^[A-Za-z0-9._:/@+[\]-]+$/;

/** A model id as the backend accepts it: 1 to 160 safe characters, no leading `-`. */
export function validModelId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= MAX_ID_CHARS &&
    !value.startsWith("-") &&
    MODEL_ID.test(value)
  );
}

function isServerId(value: unknown): value is LocalServerId {
  return LOCAL_SERVERS.some((id) => id === value);
}

function isSource(value: unknown): value is ModelSource {
  return value === "provider" || isServerId(value);
}

function field(value: object, name: string): unknown {
  return Reflect.get(value, name);
}

/** A shown name: trimmed, at most 160 characters; `fallback` when there is none. */
function labelOf(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  const text = value.trim();
  return text === "" ? fallback : Array.from(text).slice(0, MAX_ID_CHARS).join("");
}

/** A session's model choice as the backend stores it; anything else is `null`. */
export function parseSessionModel(value: unknown): SessionModel | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const source = field(value, "source");
  const id = field(value, "id");
  return isSource(source) && validModelId(id) ? { source, id } : null;
}

function parseServer(value: unknown): LocalServer | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const running = field(value, "running");
  const list = field(value, "models");
  if (typeof running !== "boolean") return null;
  const models: LocalModel[] = [];
  const seen = new Set<string>();
  if (running && Array.isArray(list)) {
    const items: readonly unknown[] = list;
    for (const item of items) {
      if (models.length >= MAX_MODELS) break;
      if (typeof item !== "object" || item === null) continue;
      const id = field(item, "id");
      if (!validModelId(id) || seen.has(id)) continue;
      seen.add(id);
      models.push({
        id,
        label: labelOf(field(item, "label"), id),
        loaded: field(item, "loaded") === true,
      });
    }
  }
  return { running, models };
}

/**
 * The answer of `local_llm_status` and the payload of `local-llm-changed`. A malformed server
 * counts as off; `null` only when the value is no state at all. Broken models are dropped, not the
 * server, and a server that is off holds no models.
 */
export function parseLocalLlmState(value: unknown): LocalLlmState | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const off: LocalServer = { running: false, models: [] };
  const ollama = parseServer(field(value, "ollama"));
  const lmstudio = parseServer(field(value, "lmstudio"));
  if (ollama === null && lmstudio === null) return null;
  return { ollama: ollama ?? off, lmstudio: lmstudio ?? off };
}

function sameServer(a: LocalServer, b: LocalServer): boolean {
  return (
    a.running === b.running &&
    a.models.length === b.models.length &&
    a.models.every((model, index) => {
      const other = b.models[index];
      return (
        other !== undefined &&
        model.id === other.id &&
        model.label === other.label &&
        model.loaded === other.loaded
      );
    })
  );
}

export function sameLocalLlmState(a: LocalLlmState | null, b: LocalLlmState | null): boolean {
  if (a === null || b === null) return a === b;
  return LOCAL_SERVERS.every((id) => sameServer(a[id], b[id]));
}

/**
 * Holds the last state the backend sent; `null` until the first answer (outside Tauri for good),
 * so the top bar shows nothing rather than a guess. Listeners hear only real changes.
 */
export class LocalLlmStore {
  private current: LocalLlmState | null = null;
  private readonly listeners = new Set<() => void>();

  /** Takes a backend value; malformed values and unchanged states are ignored. */
  accept(value: unknown): void {
    const next = parseLocalLlmState(value);
    if (next === null || sameLocalLlmState(this.current, next)) return;
    this.current = next;
    for (const listener of this.listeners) listener();
  }

  state(): LocalLlmState | null {
    return this.current;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }
}

/** The program's vendor models, checked: valid ids, each once, a label to show. */
export function profileModels(profile: Pick<Profile, "models">): ProfileModel[] {
  const list: unknown = profile.models;
  if (!Array.isArray(list)) return [];
  const items: readonly unknown[] = list;
  const models: ProfileModel[] = [];
  const seen = new Set<string>();
  for (const item of items) {
    if (typeof item !== "object" || item === null) continue;
    const id = field(item, "id");
    if (!validModelId(id) || seen.has(id)) continue;
    seen.add(id);
    models.push({ id, label: labelOf(field(item, "label"), id) });
  }
  return models;
}

/** The local servers the program can run against, in the top bar's order. */
export function profileLocalSources(profile: Pick<Profile, "localSources">): LocalServerId[] {
  const list: unknown = profile.localSources;
  if (!Array.isArray(list)) return [];
  const items: readonly unknown[] = list;
  return LOCAL_SERVERS.filter((id) => items.includes(id));
}

/** "Ollama runs · 4 models" / "Ollama is off": the badge's tooltip and accessible name. */
export function serverText(id: LocalServerId, server: LocalServer, t: Translate): string {
  const name = SERVER_NAMES[id];
  if (!server.running) return t("localLlm.off", { name });
  const count = server.models.length;
  const models =
    count === 0
      ? t("localLlm.noModels")
      : count === 1
        ? t("localLlm.oneModel")
        : t("localLlm.models", { count });
  return `${t("localLlm.on", { name })} · ${models}`;
}

export interface ModelEntry {
  model: SessionModel;
  label: string;
  loaded: boolean;
}

export interface ModelSection {
  source: ModelSource;
  /** The program's name for its own models, else the server's name. */
  heading: string;
  entries: ModelEntry[];
}

/**
 * What the "›" menu of a program row offers: the vendor's models first (headed by the program's
 * name), then each local server the program can use that runs and holds models. Sections without
 * entries are left out; empty means the row gets no "›".
 */
export function modelSections(
  profile: Pick<Profile, "name" | "models" | "localSources">,
  local: LocalLlmState | null,
): ModelSection[] {
  const sections: ModelSection[] = [];
  const own = profileModels(profile);
  if (own.length > 0) {
    sections.push({
      source: "provider",
      heading: profile.name,
      entries: own.map((model) => ({
        model: { source: "provider", id: model.id },
        label: model.label,
        loaded: false,
      })),
    });
  }
  if (local === null) return sections;
  for (const id of profileLocalSources(profile)) {
    const server = local[id];
    if (!server.running || server.models.length === 0) continue;
    sections.push({
      source: id,
      heading: SERVER_NAMES[id],
      entries: server.models.map((model) => ({
        model: { source: id, id: model.id },
        label: model.label,
        loaded: model.loaded,
      })),
    });
  }
  return sections;
}

/**
 * How a session's model reads in its peek: "Model: Opus 5.5" for a vendor model (its label when
 * the program still lists it), "Model: qwen3:8b · Ollama" for a local one. `null` without a choice.
 */
export function modelNote(
  model: SessionModel | null | undefined,
  profile: Pick<Profile, "models"> | undefined,
  t: Translate,
): string | null {
  if (model === null || model === undefined) return null;
  if (model.source === "provider") {
    const label =
      (profile === undefined ? undefined : profileModels(profile).find((m) => m.id === model.id))
        ?.label ?? model.id;
    return t("model.note", { model: label });
  }
  return t("model.note", { model: `${model.id} · ${SERVER_NAMES[model.source]}` });
}

/**
 * The session as the backend answered, with the model it really started with. The backend's
 * answer wins, `null` included: a restore whose stored model is gone starts without it, and the
 * session must not keep claiming it. Only an answer without the field (an older backend) keeps
 * the one the window asked for.
 */
export function withModel(info: SessionInfo, asked: SessionModel | null): SessionInfo {
  return { ...info, model: info.model === undefined ? asked : parseSessionModel(info.model) };
}
