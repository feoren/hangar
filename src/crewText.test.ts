import { describe, expect, it } from "vitest";

import {
  baseName,
  bubbleText,
  bubbleTyping,
  clip,
  MAX_TARGET,
  shortTarget,
  shortTool,
} from "./crewText";
import { createTranslator } from "./i18n";
import type { SubAgent } from "./subAgents";

const en = createTranslator("en");
const de = createTranslator("de");

function agent(over: Partial<SubAgent>): SubAgent {
  return {
    id: "a",
    description: null,
    agentType: null,
    spawnDepth: null,
    status: "running",
    startedAt: null,
    lastActivity: null,
    tool: null,
    target: null,
    note: null,
    tokens: null,
    excerpt: null,
    ...over,
  };
}

const say = (tool: string | null, target: string | null, t = en) =>
  bubbleText(agent({ tool, target }), "working", t);

describe("bubble text", () => {
  it("says what the newest tool call does, with the file name only", () => {
    expect(say("Read", "C:\\src\\terminal.ts")).toBe("Reading terminal.ts");
    expect(say("Read", "/home/me/src/terminal.ts")).toBe("Reading terminal.ts");
    expect(say("Grep", "trackTyped")).toBe("Searching for trackTyped");
    expect(say("Edit", "src/SessionLook.tsx")).toBe("Editing SessionLook.tsx");
    expect(say("Write", "notes.md")).toBe("Writing notes.md");
    expect(say("Bash", "cargo test")).toBe("Running cargo test");
    expect(say("PowerShell", "cargo test")).toBe("Running cargo test");
  });

  it("speaks German too, in the first person like the figure doing the work", () => {
    expect(say("Read", "src/terminal.ts", de)).toBe("Lese terminal.ts");
    expect(say("Bash", "cargo test", de)).toBe("Führe cargo test aus");
    expect(say("Bash", null, de)).toBe("Führe einen Befehl aus");
  });

  it("says it without a target when texts are off", () => {
    expect(say("Read", null)).toBe("Reading a file");
    expect(say("Bash", null)).toBe("Running a command");
    expect(say("Grep", null)).toBe("Searching the project");
  });

  it("names an unknown tool neutrally, MCP names by their last part", () => {
    expect(say("Mystery", "x")).toBe("Using Mystery");
    expect(say("mcp__files__list_dir", null)).toBe("Using list_dir");
    expect(shortTool("A".repeat(40)).length).toBe(24);
  });

  it("starts, waits, finishes and goes quiet", () => {
    expect(say(null, null)).toBe("Starting …");
    expect(bubbleText(agent({ tool: "Read" }), "waiting", en)).toBe("I need your OK");
    expect(bubbleText(agent({ tool: "Read" }), "done", en)).toBe("Done");
    expect(bubbleText(agent({}), "quiet", en)).toBe("Quiet for over a minute");
    expect(bubbleText(agent({ tool: "Read" }), "waiting", de)).toBe("Brauche dein Okay");
  });

  it("shortens long commands to their first line and a fixed length", () => {
    const long = `cargo test --manifest-path src-tauri/Cargo.toml --all-targets\nsecond`;
    const text = say("Bash", long);
    expect(text).toBe(
      `Running ${clip("cargo test --manifest-path src-tauri/Cargo.toml --all-targets", MAX_TARGET)}`,
    );
    expect(Array.from(shortTarget("run", long)).length).toBeLessThanOrEqual(MAX_TARGET);
    expect(shortTarget("run", long)).not.toContain("second");
  });

  it("cuts only what is too long", () => {
    expect(clip("short", 10)).toBe("short");
    expect(clip("abcdefghij", 5)).toBe("abcd…");
    expect(baseName("a\\b\\c.txt")).toBe("c.txt");
    expect(baseName("dir/")).toBe("dir");
    expect(baseName("plain")).toBe("plain");
  });
});

describe("typing dots", () => {
  it("stand in for the start text of a working figure", () => {
    expect(bubbleTyping("working", say(null, null), en)).toBe(true);
    expect(bubbleTyping("working", say(null, null, de), de)).toBe(true);
  });

  it("give way to words once a tool is used", () => {
    expect(bubbleTyping("working", say("Read", "src/a.ts"), en)).toBe(false);
    expect(bubbleTyping("working", say("Grep", null), en)).toBe(false);
  });

  it("follow the shown text, not the newest one", () => {
    // A held back start text (BubbleThrottle) keeps its dots; a held back tool text keeps words.
    expect(bubbleTyping("working", en("subAgents.starting"), en)).toBe(true);
    expect(bubbleTyping("working", "Reading a file", en)).toBe(false);
  });

  it("never show in another state", () => {
    for (const state of ["waiting", "done", "quiet"] as const) {
      expect(bubbleTyping(state, en("subAgents.starting"), en)).toBe(false);
    }
  });
});
