// The model flyout of a program row (ADR 0022): its rows and where it goes. Opening it on hover
// and with the keyboard is tested with the row (ProgramRow.test.tsx).

import { describe, expect, it, vi } from "vitest";

import type { SessionModel } from "./api";
import { createTranslator } from "./i18n";
import { modelSections, type LocalLlmState } from "./localLlm";
import { flyoutPlace, modelMenuItems } from "./ModelMenu";

const t = createTranslator("en");

const LOCAL: LocalLlmState = {
  ollama: {
    running: true,
    models: [{ id: "qwen3:8b", label: "qwen3:8b", loaded: true }],
  },
  lmstudio: {
    running: true,
    models: [{ id: "qwen/qwen3-8b", label: "Qwen3 8B", loaded: false }],
  },
};

const SECTIONS = modelSections(
  {
    name: "Claude Code",
    models: [{ id: "opus", label: "Opus 5.5" }],
    localSources: ["ollama", "lmstudio"],
  },
  LOCAL,
);

describe("modelMenuItems", () => {
  it("heads each section and marks loaded local models quietly", () => {
    const items = modelMenuItems("Claude Code", SECTIONS, vi.fn(), t);
    expect(
      items.map((item) =>
        item.type === "heading"
          ? `# ${item.label}`
          : item.type === "action"
            ? [item.label, item.detail].filter(Boolean).join(" / ")
            : item.type,
      ),
    ).toEqual([
      "# Claude Code",
      "Opus 5.5",
      "# Ollama",
      "qwen3:8b / loaded",
      "# LM Studio",
      "Qwen3 8B",
    ]);
  });

  it("starts with the picked model", () => {
    const onPick = vi.fn<(model: SessionModel) => void>();
    const items = modelMenuItems("Claude Code", SECTIONS, onPick, t);
    const qwen = items.find((item) => item.type === "action" && item.label === "Qwen3 8B");
    if (qwen?.type === "action") qwen.run();
    expect(onPick).toHaveBeenCalledWith({ source: "lmstudio", id: "qwen/qwen3-8b" });
    const opus = items.find((item) => item.type === "action" && item.label === "Opus 5.5");
    expect(opus?.type === "action" ? opus.title : null).toBe("Start Claude Code with Opus 5.5");
  });
});

describe("flyoutPlace", () => {
  it("goes right of the row, level with its top, and ends at its bottom when flipped", () => {
    expect(flyoutPlace({ right: 260, top: 400, bottom: 430 })).toEqual({
      x: 264,
      y: 400,
      flipAbove: 430,
    });
  });
});
