import { describe, expect, it, vi } from "vitest";

import type { Profile, SessionInfo } from "./api";
import { createTranslator } from "./i18n";
import {
  effortLabel,
  LocalLlmStore,
  modelBadge,
  modelNote,
  modelSections,
  parseLocalLlmState,
  parseSessionModel,
  profileLocalSources,
  profileModels,
  sameLocalLlmState,
  serverText,
  validModelId,
  withModel,
  type LocalLlmState,
} from "./localLlm";

const en = createTranslator("en");
const de = createTranslator("de");

const RUNNING: LocalLlmState = {
  ollama: {
    running: true,
    models: [
      { id: "qwen3:8b", label: "qwen3:8b", loaded: true },
      { id: "llama3.2:latest", label: "llama3.2:latest", loaded: false },
    ],
  },
  lmstudio: { running: false, models: [] },
};

const CLAUDE: Pick<Profile, "name" | "models" | "localSources"> = {
  name: "Claude Code",
  models: [
    { id: "opus", label: "Opus 5.5" },
    { id: "sonnet", label: "Sonnet 5.5" },
  ],
  localSources: ["ollama", "lmstudio"],
};

describe("validModelId", () => {
  it("takes the ids the backend takes", () => {
    for (const id of ["opus", "qwen3:8b", "qwen/qwen3-8b", "gpt-6.2-sol", "a@b+c[1]", "x_y"]) {
      expect(validModelId(id), id).toBe(true);
    }
    expect(validModelId("x".repeat(160))).toBe(true);
  });

  it("refuses empty, long, flag-like and unsafe ids", () => {
    for (const id of ["", "-m", "a b", "a;b", "a&b", "a\nb", "x".repeat(161), 3, null]) {
      expect(validModelId(id), String(id)).toBe(false);
    }
  });
});

describe("parseSessionModel", () => {
  it("reads a choice and drops anything else", () => {
    expect(parseSessionModel({ source: "provider", id: "opus" })).toEqual({
      source: "provider",
      id: "opus",
    });
    expect(parseSessionModel({ source: "ollama", id: "qwen3:8b" })?.source).toBe("ollama");
    expect(parseSessionModel({ source: "cloud", id: "opus" })).toBeNull();
    expect(parseSessionModel({ source: "lmstudio", id: "--rm" })).toBeNull();
    expect(parseSessionModel({ source: "provider", id: "opus", effort: "high" })).toEqual({
      source: "provider",
      id: "opus",
      effort: "high",
    });
    // A bad effort costs only the effort.
    expect(parseSessionModel({ source: "provider", id: "opus", effort: "a b" })).toEqual({
      source: "provider",
      id: "opus",
    });
    expect(parseSessionModel(null)).toBeNull();
    expect(parseSessionModel(["provider", "opus"])).toBeNull();
  });
});

describe("parseLocalLlmState", () => {
  it("reads both servers", () => {
    const value = {
      ollama: { running: true, models: [{ id: "qwen3:8b", label: "qwen3:8b", loaded: true }] },
      lmstudio: {
        running: true,
        models: [{ id: "qwen/qwen3-8b", label: "Qwen3 8B", loaded: false }],
      },
    };
    expect(parseLocalLlmState(value)).toEqual(value);
  });

  it("drops broken models, not the server", () => {
    const state = parseLocalLlmState({
      ollama: {
        running: true,
        models: [
          { id: "ok:1" },
          { id: "-bad" },
          { id: "ok:1", label: "twice" },
          "text",
          { id: "named", label: "  ", loaded: "yes" },
        ],
      },
      lmstudio: { running: false, models: [{ id: "ghost" }] },
    });
    expect(state?.ollama.models).toEqual([
      { id: "ok:1", label: "ok:1", loaded: false },
      { id: "named", label: "named", loaded: false },
    ]);
    // A server that is off holds no models.
    expect(state?.lmstudio).toEqual({ running: false, models: [] });
  });

  it("counts a malformed server as off and keeps at most 200 models", () => {
    const many = Array.from({ length: 250 }, (_, index) => ({ id: `m${String(index)}` }));
    const state = parseLocalLlmState({ ollama: { running: true, models: many }, lmstudio: 7 });
    expect(state?.ollama.models).toHaveLength(200);
    expect(state?.lmstudio).toEqual({ running: false, models: [] });
  });

  it("is null for anything that is no state", () => {
    expect(parseLocalLlmState(null)).toBeNull();
    expect(parseLocalLlmState([])).toBeNull();
    expect(parseLocalLlmState({})).toBeNull();
    expect(parseLocalLlmState({ ollama: { running: "yes" } })).toBeNull();
  });
});

describe("LocalLlmStore", () => {
  it("is unknown until the first answer and tells only real changes", () => {
    const store = new LocalLlmStore();
    const listener = vi.fn();
    const stop = store.subscribe(listener);
    expect(store.state()).toBeNull();
    store.accept("garbage");
    expect(store.state()).toBeNull();
    expect(listener).not.toHaveBeenCalled();
    store.accept(RUNNING);
    expect(store.state()).toEqual(RUNNING);
    store.accept(structuredClone(RUNNING));
    expect(listener).toHaveBeenCalledTimes(1);
    store.accept({ ...RUNNING, lmstudio: { running: true, models: [] } });
    expect(listener).toHaveBeenCalledTimes(2);
    stop();
    store.accept(RUNNING);
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("compares states by content", () => {
    expect(sameLocalLlmState(null, null)).toBe(true);
    expect(sameLocalLlmState(RUNNING, null)).toBe(false);
    const loaded = structuredClone(RUNNING);
    loaded.ollama.models[1] = { id: "llama3.2:latest", label: "llama3.2:latest", loaded: true };
    expect(sameLocalLlmState(RUNNING, loaded)).toBe(false);
  });
});

describe("serverText", () => {
  it("says in words whether the server runs and how many models it has", () => {
    expect(serverText("ollama", RUNNING.ollama, de)).toBe("Ollama läuft · 2 Modelle");
    expect(serverText("ollama", RUNNING.ollama, en)).toBe("Ollama is running · 2 models");
    expect(serverText("lmstudio", RUNNING.lmstudio, de)).toBe("LM Studio ist aus");
    expect(serverText("lmstudio", RUNNING.lmstudio, en)).toBe("LM Studio is off");
    const one = { running: true, models: RUNNING.ollama.models.slice(0, 1) };
    expect(serverText("ollama", one, de)).toBe("Ollama läuft · 1 Modell");
    expect(serverText("lmstudio", { running: true, models: [] }, en)).toBe(
      "LM Studio is running · no models",
    );
  });
});

describe("profile fields", () => {
  it("reads the vendor models and local sources defensively", () => {
    expect(profileModels({})).toEqual([]);
    expect(
      profileModels({
        models: [
          { id: "opus", label: "Opus 5.5" },
          { id: "opus", label: "again" },
          { id: "-x", label: "bad" },
          { id: "haiku", label: "" },
        ],
      }),
    ).toEqual([
      { id: "opus", label: "Opus 5.5" },
      { id: "haiku", label: "haiku" },
    ]);
    expect(
      profileModels({
        models: [
          {
            id: "opus",
            label: "Opus",
            efforts: ["low", "low", "High", "-x", "a b", "max"],
            defaultEffort: "max",
          },
          { id: "sonnet", label: "Sonnet", efforts: ["low"], defaultEffort: "ultra" },
          { id: "haiku", label: "Haiku", efforts: [] },
        ],
      }),
    ).toEqual([
      { id: "opus", label: "Opus", efforts: ["low", "max"], defaultEffort: "max" },
      { id: "sonnet", label: "Sonnet", efforts: ["low"] },
      { id: "haiku", label: "Haiku" },
    ]);
    expect(profileLocalSources({})).toEqual([]);
    expect(profileLocalSources({ localSources: ["lmstudio", "ollama"] })).toEqual([
      "ollama",
      "lmstudio",
    ]);
  });
});

describe("modelSections", () => {
  it("lists the program's models first, then only running servers with models", () => {
    const sections = modelSections(CLAUDE, RUNNING);
    expect(sections.map((section) => [section.source, section.heading])).toEqual([
      ["provider", "Claude Code"],
      ["ollama", "Ollama"],
    ]);
    expect(sections[0]?.entries.map((entry) => entry.model)).toEqual([
      { source: "provider", id: "opus" },
      { source: "provider", id: "sonnet" },
    ]);
    expect(sections[1]?.entries.map((entry) => [entry.model.id, entry.loaded])).toEqual([
      ["qwen3:8b", true],
      ["llama3.2:latest", false],
    ]);
  });

  it("offers local models only to programs that can use that server", () => {
    expect(modelSections({ ...CLAUDE, localSources: ["lmstudio"] }, RUNNING)).toHaveLength(1);
    expect(modelSections({ name: "Aider", localSources: ["ollama"] }, RUNNING)).toHaveLength(1);
  });

  it("is empty without any choice, so the row gets no menu", () => {
    expect(modelSections({ name: "PowerShell" }, RUNNING)).toEqual([]);
    expect(modelSections({ name: "Aider", localSources: ["ollama"] }, null)).toEqual([]);
    const empty: LocalLlmState = {
      ollama: { running: true, models: [] },
      lmstudio: { running: false, models: [] },
    };
    expect(modelSections({ name: "Aider", localSources: ["ollama", "lmstudio"] }, empty)).toEqual(
      [],
    );
  });
});

describe("modelNote", () => {
  it("names a vendor model by its label and a local one with its server", () => {
    expect(modelNote({ source: "provider", id: "opus" }, CLAUDE, de)).toBe("Modell: Opus 5.5");
    expect(modelNote({ source: "provider", id: "gone" }, CLAUDE, en)).toBe("Model: gone");
    expect(modelNote({ source: "ollama", id: "qwen3:8b" }, CLAUDE, en)).toBe(
      "Model: qwen3:8b · Ollama",
    );
    expect(modelNote({ source: "lmstudio", id: "qwen/qwen3-8b" }, undefined, en)).toBe(
      "Model: qwen/qwen3-8b · LM Studio",
    );
    expect(modelNote(null, CLAUDE, en)).toBeNull();
    expect(modelNote(undefined, CLAUDE, en)).toBeNull();
  });

  it("names a chosen effort after the model", () => {
    expect(modelNote({ source: "provider", id: "opus", effort: "high" }, CLAUDE, de)).toBe(
      "Modell: Opus 5.5 · Hoch",
    );
    expect(modelNote({ source: "provider", id: "opus", effort: "xhigh" }, CLAUDE, en)).toBe(
      "Model: Opus 5.5 · Extra high",
    );
  });
});

describe("modelBadge", () => {
  it("shows the model and, when chosen, its effort", () => {
    expect(modelBadge({ source: "provider", id: "opus" }, CLAUDE, de)).toBe("Opus 5.5");
    expect(modelBadge({ source: "provider", id: "opus", effort: "high" }, CLAUDE, de)).toBe(
      "Opus 5.5 · Hoch",
    );
    // A long name is cut, the effort still shows.
    const long = { source: "provider", id: "x".repeat(40), effort: "max" } as const;
    expect(modelBadge(long, undefined, de)).toBe(`${"x".repeat(21)}… · Maximal`);
    expect(modelBadge({ source: "ollama", id: "hf.co/org/Qwen3:Q4" }, CLAUDE, de)).toBe("Qwen3:Q4");
    expect(modelBadge(null, CLAUDE, de)).toBeNull();
  });
});

describe("effortLabel", () => {
  it("names the known levels in the language and any other as the program does", () => {
    expect(
      ["low", "medium", "high", "xhigh", "max", "ultra"].map((effort) => effortLabel(effort, de)),
    ).toEqual(["Niedrig", "Mittel", "Hoch", "Extra hoch", "Maximal", "Ultra"]);
    expect(effortLabel("turbo", de)).toBe("turbo");
    expect(effortLabel("constructor", en)).toBe("constructor");
  });
});

describe("withModel", () => {
  const info: SessionInfo = {
    id: 1,
    profileId: "claude",
    title: "Claude Code",
    cwd: "C:\\work",
    status: "running",
    exitCode: null,
    createdAt: 0,
  };

  it("keeps the backend's answer, else the one asked for", () => {
    const opus = { source: "provider", id: "opus" } as const;
    const local = { source: "ollama", id: "qwen3:8b" } as const;
    expect(withModel({ ...info, model: local }, opus).model).toEqual(local);
    expect(withModel(info, opus).model).toEqual(opus);
    expect(withModel(info, null).model).toBeNull();
  });

  it("drops a stored model the backend started without", () => {
    const gone = { source: "provider", id: "opus-4" } as const;
    expect(withModel({ ...info, model: null }, gone).model).toBeNull();
  });
});
