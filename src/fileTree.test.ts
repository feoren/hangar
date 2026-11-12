import { describe, expect, it } from "vitest";

import type { FolderEntry, FolderListing } from "./api";
import type { Claim } from "./claims";
import {
  ancestors,
  avatarStack,
  buildMarks,
  folderKeyOf,
  foldersToRefresh,
  INITIAL_TREE,
  isBusyError,
  repoToTreeKey,
  rootOffset,
  treeReducer,
  treeStep,
  visibleRows,
  type ItemRow,
  type TeamEditor,
  type TreeAction,
  type TreeState,
} from "./fileTree";
import { peersOf } from "./repoPeers";
import { EMPTY_ACTIVITY, type PeerSession, type PeerView, type TeamActivity } from "./team";

const dir = (name: string): FolderEntry => ({ name, kind: "dir", ignored: false });
const file = (name: string, ignored = false): FolderEntry => ({ name, kind: "file", ignored });
const link = (name: string): FolderEntry => ({ name, kind: "symlink", ignored: false });

function listing(entries: FolderEntry[], extra: Partial<FolderListing> = {}): FolderListing {
  return { root: "C:\\src\\shop", repoRoot: "C:\\src\\shop", entries, truncated: false, ...extra };
}

function run(actions: TreeAction[], state: TreeState = INITIAL_TREE): TreeState {
  return actions.reduce(treeReducer, state);
}

/** Root commits of two repositories one project links. */
const SHOP = "a".repeat(40);
const DOCS = "d".repeat(40);

function peer(member: string, files: string[], extra: Partial<PeerView> = {}): PeerView {
  return {
    member,
    device: `${member}-device`,
    name: member.toUpperCase(),
    own: false,
    presence: "active",
    sessions: [
      {
        key: `${member}-1`,
        agent: "Claude Code",
        title: "",
        state: "working",
        repo: SHOP,
        files,
        moreFiles: 0,
      },
    ],
    ...extra,
  };
}

const claim = (path: string, kind: Claim["kind"] = "modified"): Claim => ({
  path,
  kind,
  wholeFile: false,
  ranges: [],
  area: null,
});

describe("paths", () => {
  it("normalizes Windows folders without case, prefix and trailing separator", () => {
    expect(folderKeyOf("C:\\Src\\Shop\\")).toEqual({ key: "c:/src/shop", windows: true });
    expect(folderKeyOf("c:/src/shop")).toEqual({ key: "c:/src/shop", windows: true });
    expect(folderKeyOf("\\\\?\\C:\\Src")).toEqual({ key: "c:/src", windows: true });
    expect(folderKeyOf("\\\\?\\UNC\\Host\\Share\\x")).toEqual({
      key: "//host/share/x",
      windows: true,
    });
    expect(folderKeyOf("C:\\")).toEqual({ key: "c:/", windows: true });
  });

  it("keeps case and backslashes on POSIX", () => {
    expect(folderKeyOf("/home/Ann/Shop/")).toEqual({ key: "/home/Ann/Shop", windows: false });
    expect(folderKeyOf("/home/a\\b")).toEqual({ key: "/home/a\\b", windows: false });
    expect(folderKeyOf("/")).toEqual({ key: "/", windows: false });
  });

  it("finds the root's place inside the repository", () => {
    expect(rootOffset("C:\\src\\shop", "c:/src/shop")).toBe("");
    expect(rootOffset("C:\\Src\\Shop\\Web", "c:/src/shop")).toBe("web");
    expect(rootOffset("/r/shop/web/app", "/r/shop")).toBe("web/app");
    expect(rootOffset("/r/shopping", "/r/shop")).toBeNull();
    expect(rootOffset("/r/other", "/r/shop")).toBeNull();
    expect(rootOffset("C:\\x", "C:\\")).toBe("x");
    expect(rootOffset("/r/shop", null)).toBeNull();
  });

  it("maps repository paths onto the tree and drops files outside its root", () => {
    expect(repoToTreeKey("src/App.tsx", "", true)).toBe("src/app.tsx");
    expect(repoToTreeKey("src/App.tsx", "", false)).toBe("src/App.tsx");
    expect(repoToTreeKey("web/src/main.ts", "web", false)).toBe("src/main.ts");
    expect(repoToTreeKey("Web/Main.ts", "web", true)).toBe("main.ts");
    expect(repoToTreeKey("api/main.ts", "web", false)).toBeNull();
    expect(repoToTreeKey("website/x.ts", "web", false)).toBeNull();
    expect(repoToTreeKey("web", "web", false)).toBeNull();
    expect(repoToTreeKey("", "", false)).toBeNull();
  });

  it("lists the folders containing a path", () => {
    expect(ancestors("a/b/c.txt")).toEqual(["a", "a/b"]);
    expect(ancestors("top.txt")).toEqual([]);
  });
});

describe("markings", () => {
  it("marks nothing outside a repository", () => {
    const marks = buildMarks({
      root: "/r/shop",
      repoRoot: null,
      peers: [peer("ann", ["a.txt"])],
      changes: [{ root: "/r/shop", claims: [claim("a.txt")] }],
      overlaps: [],
    });
    expect(marks.files.size).toBe(0);
    expect(marks.folders.size).toBe(0);
  });

  it("puts teammates on their files and the containing folders", () => {
    const marks = buildMarks({
      root: "C:\\src\\shop\\web",
      repoRoot: "C:\\src\\shop",
      peers: [peer("ann", ["web/src/App.tsx", "api/main.rs"]), peer("bob", ["Web/src/app.tsx"])],
      changes: [],
      overlaps: [],
    });
    expect([...marks.files.keys()]).toEqual(["src/app.tsx"]);
    expect(marks.files.get("src/app.tsx")?.editors.map((e) => e.member)).toEqual(["ann", "bob"]);
    expect(marks.folders.get("src")).toEqual({ team: true, change: false });
  });

  it("counts a person with the same agent once per file", () => {
    const session = {
      agent: "Claude Code",
      title: "",
      state: "working" as const,
      repo: SHOP,
      moreFiles: 0,
    };
    const twice = peer("ann", [], {
      sessions: [
        { ...session, key: "ann-1", files: ["a.txt"] },
        { ...session, key: "ann-2", files: ["a.txt"] },
      ],
    });
    const marks = buildMarks({
      root: "/r",
      repoRoot: "/r",
      peers: [twice],
      changes: [],
      overlaps: [],
    });
    expect(marks.files.get("a.txt")?.editors).toHaveLength(1);
  });

  it("shows the worktree's own changes, the worst kind first, and ignores other worktrees", () => {
    const marks = buildMarks({
      root: "C:\\src\\shop",
      repoRoot: "C:\\src\\shop",
      peers: [],
      changes: [
        {
          root: "c:/src/shop",
          claims: [claim("a.txt", "untracked"), claim("lib/gone.ts", "deleted")],
        },
        { root: "C:/Src/Shop", claims: [claim("A.txt", "conflicted")] },
        { root: "c:/src/shop-wt", claims: [claim("b.txt")] },
        { root: null, claims: [claim("c.txt")] },
      ],
      overlaps: [
        { path: "a.txt", session: 7 },
        { path: "A.txt", session: 7 },
      ],
    });
    expect(marks.files.get("a.txt")).toEqual({
      editors: [],
      change: "conflicted",
      alsoIn: [7],
    });
    expect(marks.files.has("lib/gone.ts")).toBe(false);
    expect(marks.folders.get("lib")).toEqual({ team: false, change: true });
    expect(marks.files.has("b.txt")).toBe(false);
  });

  it("stacks avatars per person with the rest as a count", () => {
    const editor = (member: string, agent = "Codex"): TeamEditor => ({
      member,
      device: "d",
      name: member,
      own: false,
      agent,
    });
    const stack = avatarStack(
      [editor("ann"), editor("ann", "Aider"), editor("bob"), editor("cid"), editor("dan")],
      2,
    );
    expect(stack.shown.map((e) => e.member)).toEqual(["ann", "bob"]);
    expect(stack.more).toBe(2);
    expect(avatarStack([editor("ann")], 2)).toEqual({ shown: [editor("ann")], more: 0 });
  });

  it("takes the teammates of every project the session is shared with, each device once", () => {
    const shop = (...ids: number[]) => ids.map((sessionId) => ({ sessionId, repo: SHOP }));
    const shared: TeamActivity = {
      ...EMPTY_ACTIVITY,
      sessions: [3],
      sessionRepos: shop(3),
      peers: [peer("ann", [])],
    };
    const again: TeamActivity = {
      ...EMPTY_ACTIVITY,
      sessions: [3, 4],
      sessionRepos: shop(3, 4),
      peers: [peer("ann", []), peer("bob", [])],
    };
    const other: TeamActivity = {
      ...EMPTY_ACTIVITY,
      sessions: [4],
      sessionRepos: shop(4),
      peers: [peer("cid", [])],
    };
    const activities = new Map([
      ["p1", shared],
      ["p2", again],
      ["p3", other],
    ]);
    expect(peersOf(activities, 3).map((p) => p.member)).toEqual(["ann", "bob"]);
    expect(peersOf(activities, 9)).toEqual([]);
  });

  it("keeps only the teammates' sessions in the session's own repository", () => {
    // One project links the shop and its docs: both have a README.md at the top.
    const readme = (key: string, repo: string): PeerSession => ({
      key,
      agent: "Claude Code",
      title: "",
      state: "working",
      repo,
      files: ["README.md"],
      moreFiles: 0,
    });
    const ann = peer("ann", [], {
      sessions: [readme("ann-shop", SHOP), readme("ann-docs", DOCS)],
    });
    const bob = peer("bob", [], { sessions: [readme("bob-1", DOCS)] });
    const project: TeamActivity = {
      ...EMPTY_ACTIVITY,
      sessions: [3, 4],
      sessionRepos: [
        { sessionId: 3, repo: SHOP },
        { sessionId: 4, repo: DOCS },
      ],
      peers: [ann, bob],
    };
    const activities = new Map([["p", project]]);
    const inShop = peersOf(activities, 3);
    expect(inShop.map((p) => [p.member, p.sessions.map((s) => s.key)])).toEqual([
      ["ann", ["ann-shop"]],
    ]);
    const inDocs = peersOf(activities, 4);
    expect(inDocs.map((p) => [p.member, p.sessions.map((s) => s.key)])).toEqual([
      ["ann", ["ann-docs"]],
      ["bob", ["bob-1"]],
    ]);
    const marks = buildMarks({
      root: "/r",
      repoRoot: "/r",
      peers: inShop,
      changes: [],
      overlaps: [],
    });
    expect(marks.files.get("README.md")?.editors.map((e) => e.member)).toEqual(["ann"]);
    // A session the backend names no repository for (not shared) gets no teammates.
    expect(peersOf(new Map([["p", { ...project, sessionRepos: [] }]]), 3)).toEqual([]);
    // The input stays untouched.
    expect(ann.sessions).toHaveLength(2);
  });
});

describe("tree state", () => {
  const loaded = run([
    { type: "request", path: "" },
    { type: "loaded", path: "", listing: listing([dir("src"), dir("docs"), file("a.txt")]) },
  ]);

  it("loads the root and remembers where it lies", () => {
    expect(INITIAL_TREE.folders.size).toBe(0);
    expect(loaded.root).toBe("C:\\src\\shop");
    expect(loaded.repoRoot).toBe("C:\\src\\shop");
    expect(visibleRows(loaded).map((row) => (row.kind === "item" ? row.path : row.hint))).toEqual([
      "src",
      "docs",
      "a.txt",
    ]);
  });

  it("shows loading, failure, empty and truncated hints", () => {
    expect(visibleRows(INITIAL_TREE)).toEqual([
      { kind: "hint", folder: "", depth: 0, hint: "loading", message: "" },
    ]);
    const state = run(
      [
        { type: "expand", path: "src" },
        { type: "request", path: "src" },
        { type: "expand", path: "docs" },
        { type: "failed", path: "docs", message: "denied" },
      ],
      loaded,
    );
    const hints = visibleRows(state).filter((row) => row.kind === "hint");
    expect(hints).toEqual([
      { kind: "hint", folder: "src", depth: 1, hint: "loading", message: "" },
      { kind: "hint", folder: "docs", depth: 1, hint: "failed", message: "denied" },
    ]);
    const more = run(
      [
        { type: "loaded", path: "src", listing: listing([]) },
        { type: "loaded", path: "docs", listing: listing([file("x.md")], { truncated: true }) },
      ],
      state,
    );
    expect(visibleRows(more).filter((row) => row.kind === "hint")).toEqual([
      { kind: "hint", folder: "src", depth: 1, hint: "empty", message: "" },
      { kind: "hint", folder: "docs", depth: 1, hint: "truncated", message: "" },
    ]);
  });

  it("keeps what it shows while reading again, also when that fails", () => {
    const again = run(
      [
        { type: "request", path: "" },
        { type: "failed", path: "", message: "timed out" },
      ],
      loaded,
    );
    expect(again.folders.get("")?.kind).toBe("loaded");
  });

  it("nests expanded folders with positions and remembers them when collapsed", () => {
    const state = run(
      [
        { type: "expand", path: "src" },
        { type: "loaded", path: "src", listing: listing([dir("ui"), file("main.ts")]) },
        { type: "expand", path: "src/ui" },
        { type: "loaded", path: "src/ui", listing: listing([file("app.tsx")]) },
      ],
      loaded,
    );
    const items = visibleRows(state).filter((row): row is ItemRow => row.kind === "item");
    expect(items.map((row) => [row.path, row.depth, row.position, row.siblings])).toEqual([
      ["src", 0, 1, 3],
      ["src/ui", 1, 1, 2],
      ["src/ui/app.tsx", 2, 1, 1],
      ["src/main.ts", 1, 2, 2],
      ["docs", 0, 2, 3],
      ["a.txt", 0, 3, 3],
    ]);
    const collapsed = treeReducer(state, { type: "collapse", path: "src" });
    expect(visibleRows(collapsed)).toHaveLength(3);
    expect(foldersToRefresh(state)).toEqual(["", "src", "src/ui"]);
    // A folder inside a collapsed one is not on screen: no need to read it.
    expect(foldersToRefresh(collapsed)).toEqual([""]);
    const reopened = treeReducer(collapsed, { type: "expand", path: "src" });
    expect(visibleRows(reopened)).toHaveLength(6);
  });

  it("returns the same state when nothing changes", () => {
    expect(treeReducer(loaded, { type: "collapse", path: "src" })).toBe(loaded);
    expect(treeReducer(loaded, { type: "request", path: "" })).toBe(loaded);
    const focused = treeReducer(loaded, { type: "focus", path: "a.txt" });
    expect(treeReducer(focused, { type: "focus", path: "a.txt" })).toBe(focused);
  });

  it("recognizes the backend's busy answer", () => {
    expect(isBusyError("The folder is still being read")).toBe(true);
    expect(isBusyError("The folder did not answer in time")).toBe(false);
  });
});

describe("keyboard", () => {
  const state = run([
    { type: "loaded", path: "", listing: listing([dir("src"), link("out"), file("a.txt")]) },
    { type: "expand", path: "src" },
    { type: "loaded", path: "src", listing: listing([file("main.ts")]) },
  ]);
  const rows = visibleRows(state);

  it("moves up, down, home and end over the visible items", () => {
    expect(treeStep(rows, null, "ArrowDown")).toEqual({ kind: "focus", path: "src" });
    expect(treeStep(rows, "src", "ArrowDown")).toEqual({ kind: "focus", path: "src/main.ts" });
    expect(treeStep(rows, "src/main.ts", "ArrowUp")).toEqual({ kind: "focus", path: "src" });
    expect(treeStep(rows, "src", "ArrowUp")).toEqual({ kind: "none" });
    expect(treeStep(rows, "out", "Home")).toEqual({ kind: "focus", path: "src" });
    expect(treeStep(rows, "src", "End")).toEqual({ kind: "focus", path: "a.txt" });
    expect(treeStep(rows, "a.txt", "ArrowDown")).toEqual({ kind: "none" });
  });

  it("opens, enters, closes and leaves folders with the arrows", () => {
    expect(treeStep(rows, "src", "ArrowRight")).toEqual({ kind: "focus", path: "src/main.ts" });
    expect(treeStep(rows, "src", "ArrowLeft")).toEqual({ kind: "collapse", path: "src" });
    expect(treeStep(rows, "src/main.ts", "ArrowLeft")).toEqual({ kind: "focus", path: "src" });
    expect(treeStep(rows, "a.txt", "ArrowLeft")).toEqual({ kind: "none" });
    const closed = visibleRows(treeReducer(state, { type: "collapse", path: "src" }));
    expect(treeStep(closed, "src", "ArrowRight")).toEqual({ kind: "expand", path: "src" });
  });

  it("never enters a link and selects files with Enter", () => {
    expect(treeStep(rows, "out", "ArrowRight")).toEqual({ kind: "none" });
    expect(treeStep(rows, "out", "Enter")).toEqual({ kind: "select", path: "out" });
    expect(treeStep(rows, "a.txt", " ")).toEqual({ kind: "select", path: "a.txt" });
    expect(treeStep(rows, "src", "Enter")).toEqual({ kind: "collapse", path: "src" });
    expect(treeStep(rows, "src", "x")).toBeNull();
    expect(treeStep([], null, "ArrowDown")).toBeNull();
  });
});
