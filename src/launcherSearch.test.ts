import { describe, expect, it, vi } from "vitest";

import { createTranslator } from "./i18n";
import { rankEntries } from "./launcher";
import { loadProjectItems, NOTE_HITS, noteItems, searchNoteItems } from "./launcherSearch";
import type { SearchHit } from "./notes";

const t = createTranslator("en");

function hit(id: string, patch: Partial<SearchHit> = {}): SearchHit {
  return {
    folderKey: "general",
    folder: { path: null },
    id,
    title: `Note ${id}`,
    updatedAt: null,
    excerpt: "",
    ...patch,
  };
}

/** The backend's answer as `search_notes` sends it. */
function wire(hits: readonly SearchHit[]): unknown {
  return hits.map((entry) => ({
    folderKey: entry.folderKey,
    path: entry.folder.path,
    id: entry.id,
    title: entry.title,
    excerpt: entry.excerpt,
    updatedAt: entry.updatedAt,
  }));
}

describe("note entries", () => {
  it("names the note, where it lives and the passage that matched", () => {
    const open = vi.fn();
    const [entry] = noteItems(
      [
        hit("a", {
          folderKey: "0123456789abcdef0123456789abcdef",
          folder: { path: "C:\\src\\hangar" },
          title: "Release plan",
          excerpt: "  sign the\n  installer first  ",
        }),
      ],
      open,
      t,
      null,
    );
    expect(entry?.section).toBe("notes");
    expect(entry?.label).toBe("Release plan");
    expect(entry?.detail).toBe("hangar · sign the installer first");
    expect(entry?.found).toBe(true);
    entry?.run();
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ id: "a" }));
  });

  it("names untitled notes and the General area, and keeps the list short", () => {
    const [entry] = noteItems([hit("a", { title: " " })], vi.fn(), t, null);
    expect(entry?.label).toBe(t("notes.untitled"));
    expect(entry?.detail).toBe(t("notes.general"));
    const many = Array.from({ length: NOTE_HITS + 5 }, (_, index) => hit(String(index)));
    expect(noteItems(many, vi.fn(), t, null)).toHaveLength(NOTE_HITS);
  });

  it("asks the backend only with a query, and checks its answer", async () => {
    const search = vi.fn((query: string) => Promise.resolve(wire([hit(query)])));
    expect(await searchNoteItems("   ", search, vi.fn(), t, null)).toEqual([]);
    expect(search).not.toHaveBeenCalled();
    const items = await searchNoteItems(" installer ", search, vi.fn(), t, null);
    expect(search).toHaveBeenCalledWith("installer");
    expect(items.map((item) => item.key)).toEqual(["note:general:installer"]);
    const broken = await searchNoteItems("x", () => Promise.resolve("nope"), vi.fn(), t, null);
    expect(broken).toEqual([]);
  });

  it("shows a hit found in the text even where its title does not match", () => {
    const entries = noteItems([hit("a", { title: "Plan" })], vi.fn(), t, null);
    const sections = rankEntries(entries, "zebra");
    expect(sections.map((section) => section.section)).toEqual(["notes"]);
    // Fuzzy matching alone finds nothing.
    const plain = entries.map((entry) => ({ ...entry, found: false }));
    expect(rankEntries(plain, "zebra")).toEqual([]);
  });
});

describe("project entries", () => {
  const sessions = [
    { id: 1, cwd: "C:\\src\\hangar", title: "Claude" },
    { id: 2, cwd: "C:\\src\\hangar\\src-tauri", title: "Codex" },
    { id: 3, cwd: "C:\\src\\site", title: "Aider" },
    { id: 4, cwd: "C:\\Users\\me", title: "Update", kind: "update" as const },
  ];

  it("groups the sessions like the project cards and opens the chosen card", async () => {
    const places = vi.fn(() =>
      Promise.resolve({
        "C:\\src\\hangar": { root: "C:\\src\\hangar", workTree: "C:\\src\\hangar" },
        "C:\\src\\hangar\\src-tauri": { root: "C:\\src\\hangar", workTree: "C:\\src\\hangar" },
        "C:\\src\\site": null,
      }),
    );
    const open = vi.fn();
    const items = await loadProjectItems(sessions, places, open, null);
    expect(places).toHaveBeenCalledWith([
      "C:\\src\\hangar",
      "C:\\src\\hangar\\src-tauri",
      "C:\\src\\site",
    ]);
    expect(items.map((item) => [item.section, item.label])).toEqual([
      ["projects", "hangar"],
      ["projects", "site"],
    ]);
    // Found by the titles of its sessions as well.
    expect(rankEntries(items, "codex")[0]?.items[0]?.entry.label).toBe("hangar");
    items[1]?.run();
    expect(open).toHaveBeenCalledTimes(1);
    expect(open.mock.calls[0]?.[0]).toBe(items[1]?.key.slice("project:".length));
  });

  it("counts every folder as a plain one when the backend fails", async () => {
    const items = await loadProjectItems(
      sessions,
      () => Promise.reject(new Error("x")),
      vi.fn(),
      null,
    );
    expect(items.map((item) => item.label)).toEqual(["hangar", "src-tauri", "site"]);
  });

  it("asks nothing without sessions", async () => {
    const places = vi.fn(() => Promise.resolve({}));
    expect(await loadProjectItems([], places, vi.fn(), null)).toEqual([]);
    expect(places).not.toHaveBeenCalled();
  });
});
