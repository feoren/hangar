import { describe, expect, it } from "vitest";

import type { FolderEntry, FolderListing, PathEntry } from "./api";
import type { Claim } from "./claims";
import {
  mentionText,
  filteredStep,
  filterScore,
  filterTree,
  filterWords,
  loadedPaths,
  revealFolders,
  rowMention,
  startsFilter,
  takesMention,
  ancestors,
  avatarStack,
  buildMarks,
  folderKeyOf,
  folderPeople,
  foldersToRefresh,
  INITIAL_TREE,
  isBusyError,
  onChangePath,
  repoToTreeKey,
  rootOffset,
  treeReducer,
  treeStep,
  visibleRows,
  type HolderRef,
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
    const src = marks.folders.get("src");
    expect(src?.team).toBe(true);
    expect(src?.change).toBe(false);
    expect(src?.editors.map((e) => e.member)).toEqual(["ann", "bob"]);
    expect(src?.sessions).toEqual([]);
  });

  it("marks files other worktrees of the repository change, with since when", () => {
    const sinceOf = (holder: HolderRef, path: string) =>
      holder.kind === "worktree" && path === "src/a.ts" ? 1000 : null;
    const marks = buildMarks({
      root: "C:\\src\\shop",
      repoRoot: "C:\\src\\shop",
      peers: [],
      changes: [{ root: "c:/src/shop", claims: [claim("src/a.ts")] }],
      overlaps: [{ path: "src/a.ts", session: 5 }],
      others: [
        // The tree's own worktree is never "someone else".
        { root: "C:/src/shop", sessions: [9], claims: [claim("own.ts")] },
        {
          root: "c:/wt/shop-x",
          sessions: [5, 6],
          claims: [claim("src/a.ts"), claim("lib/gone.ts", "deleted")],
        },
      ],
      sinceOf,
    });
    expect(marks.files.get("src/a.ts")).toEqual({
      editors: [],
      change: "modified",
      alsoIn: [
        { session: 5, since: 1000 },
        { session: 6, since: 1000 },
      ],
    });
    expect(marks.files.has("own.ts")).toBe(false);
    expect(marks.files.has("lib/gone.ts")).toBe(false);
    expect(marks.folders.get("lib")).toEqual({
      team: true,
      change: false,
      editors: [],
      sessions: [5, 6],
    });
    const src = marks.folders.get("src");
    expect(src === undefined ? 0 : folderPeople(src)).toBe(2);
  });

  it("counts each person once and each other session on a folder", () => {
    const marks = buildMarks({
      root: "/r",
      repoRoot: "/r",
      peers: [
        peer("ann", ["docs/a.md"]),
        peer("ann", ["docs/b.md"], {
          device: "ann-laptop",
          sessions: [
            {
              key: "ann-2",
              agent: "Codex",
              title: "",
              state: "working",
              repo: SHOP,
              files: ["docs/b.md"],
              moreFiles: 0,
            },
          ],
        }),
        peer("bob", ["docs/c.md"]),
      ],
      changes: [],
      overlaps: [],
      others: [{ root: "/wt/r-1", sessions: [4], claims: [claim("docs/d.md")] }],
    });
    const docs = marks.folders.get("docs");
    expect(docs === undefined ? 0 : folderPeople(docs)).toBe(3);
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
      alsoIn: [{ session: 7, since: null }],
    });
    expect(marks.files.has("lib/gone.ts")).toBe(false);
    expect(marks.folders.get("lib")).toEqual({
      team: false,
      change: true,
      editors: [],
      sessions: [],
    });
    expect(marks.files.has("b.txt")).toBe(false);
  });

  it("stacks avatars per person with the rest as a count", () => {
    const editor = (member: string, agent = "Codex"): TeamEditor => ({
      member,
      device: "d",
      name: member,
      own: false,
      agent,
      since: null,
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

  it("opens files with Enter, selects them with Space and never opens a link", () => {
    expect(treeStep(rows, "out", "ArrowRight")).toEqual({ kind: "none" });
    expect(treeStep(rows, "out", "Enter")).toEqual({ kind: "select", path: "out" });
    expect(treeStep(rows, "a.txt", "Enter")).toEqual({ kind: "open", path: "a.txt" });
    expect(treeStep(rows, "src/main.ts", "Enter")).toEqual({ kind: "open", path: "src/main.ts" });
    expect(treeStep(rows, "a.txt", " ")).toEqual({ kind: "select", path: "a.txt" });
    expect(treeStep(rows, "src", "Enter")).toEqual({ kind: "collapse", path: "src" });
    expect(treeStep(rows, "src", "x")).toBeNull();
    expect(treeStep([], null, "ArrowDown")).toBeNull();
  });
});

describe("path of the changes", () => {
  const marks = buildMarks({
    root: "/r/shop",
    repoRoot: "/r/shop",
    peers: [peer("ann", ["docs/guide.md"])],
    changes: [
      {
        root: "/r/shop",
        claims: [claim("src/app/deep/view.ts"), claim("old/gone.ts", "deleted")],
      },
    ],
    overlaps: [],
  });
  const fileMark = (path: string) => marks.files.get(path) ?? null;
  const folderMark = (path: string) => marks.folders.get(path) ?? null;

  it("marks every folder from the top down to a changed file, whatever is expanded", () => {
    // The marks come from the claims alone; the tree's expanded folders play no part.
    for (const folder of ["src", "src/app", "src/app/deep"]) {
      expect(onChangePath("dir", null, folderMark(folder))).toBe(true);
    }
    expect(onChangePath("file", fileMark("src/app/deep/view.ts"), null)).toBe(true);
  });

  it("keeps the folder of a deleted file on the path", () => {
    expect(onChangePath("dir", null, folderMark("old"))).toBe(true);
  });

  it("leaves teammates' files, unchanged rows and links off the path", () => {
    expect(onChangePath("dir", null, folderMark("docs"))).toBe(false);
    expect(onChangePath("file", fileMark("docs/guide.md"), null)).toBe(false);
    expect(onChangePath("file", null, null)).toBe(false);
    expect(onChangePath("dir", null, null)).toBe(false);
    const changed = fileMark("src/app/deep/view.ts");
    expect(onChangePath("symlink", changed, null)).toBe(false);
  });
});

describe("mentionText", () => {
  it("puts an @ before the path from the project's folder, with a space after it", () => {
    expect(mentionText("src/app.ts", false)).toBe("@src/app.ts ");
    expect(mentionText("README.md", false)).toBe("@README.md ");
  });

  it("marks a folder with a trailing slash", () => {
    expect(mentionText("src/components", true)).toBe("@src/components/ ");
  });

  it("quotes a path with spaces so the agent reads it as one", () => {
    expect(mentionText("docs/my notes.md", false)).toBe('@"docs/my notes.md" ');
    expect(mentionText('a "b" c', false)).toBe("@'a \"b\" c' ");
  });
});

describe("rowMention", () => {
  it("is the right click's text for a dragged file or folder", () => {
    const state = run([
      { type: "loaded", path: "", listing: listing([dir("src"), file("a b.md")]) },
    ]);
    const rows = visibleRows(state).filter((row): row is ItemRow => row.kind === "item");
    expect(rows.map(rowMention)).toEqual(["@src/ ", '@"a b.md" ']);
  });
});

describe("takesMention", () => {
  it("accepts only a session in the tree's own folder", () => {
    const app = String.raw`C:\work\app`;
    expect(takesMention(app, "c:/work/app/")).toBe(true);
    expect(takesMention("/home/a/app", "/home/a/app")).toBe(true);
    expect(takesMention("/home/a/app", "/home/a/App")).toBe(false);
    expect(takesMention(app, String.raw`C:\work\app\src`)).toBe(false);
    expect(takesMention(app, String.raw`C:\work\other`)).toBe(false);
  });
});

describe("tree filter", () => {
  const paths: PathEntry[] = [
    { path: "src", kind: "dir" },
    { path: "src/main.rs", kind: "file" },
    { path: "src/lib.rs", kind: "file" },
    { path: "src/ui", kind: "dir" },
    { path: "src/ui/MainView.tsx", kind: "file" },
    { path: "docs", kind: "dir" },
    { path: "docs/main.md", kind: "file" },
    { path: "README.md", kind: "file" },
  ];

  it("splits the query into lower case words", () => {
    expect(filterWords("  Main  RS ")).toEqual(["main", "rs"]);
    expect(filterWords("   ")).toEqual([]);
  });

  it("needs every word in the path and one in the name", () => {
    expect(filterScore("src/main.rs", ["main"])).toBe(3);
    expect(filterScore("src/domain.rs", ["main"])).toBe(2);
    expect(filterScore("src/main.rs", ["src", "main"])).toBe(4);
    // Only in a folder above: not a match, or "src" would list everything inside src/.
    expect(filterScore("src/lib.rs", ["src"])).toBeNull();
    expect(filterScore("src/lib.rs", ["lib", "x"])).toBeNull();
    expect(filterScore("src", [])).toBeNull();
  });

  it("shows the matches under their folders, all open", () => {
    const result = filterTree(paths, "main");
    expect(result.matched).toBe(3);
    expect(result.rows.map((row) => [row.path, row.depth, row.expanded, row.context])).toEqual([
      ["docs", 0, true, true],
      ["docs/main.md", 1, false, false],
      ["src", 0, true, true],
      ["src/ui", 1, true, true],
      ["src/ui/MainView.tsx", 2, false, false],
      ["src/main.rs", 1, false, false],
    ]);
    const ui = result.rows.find((row) => row.path === "src/ui");
    expect(ui).toMatchObject({ parent: "src", position: 1, siblings: 2 });
  });

  it("keeps a matching folder a match, also with matches inside", () => {
    const result = filterTree(paths, "ui");
    expect(result.rows.map((row) => [row.path, row.context])).toEqual([
      ["src", true],
      ["src/ui", false],
    ]);
    expect(filterTree(paths, "").rows).toEqual([]);
    expect(filterTree(paths, "zzz")).toEqual({ rows: [], matched: 0 });
  });

  it("keeps only the best matches and counts the rest", () => {
    const many: PathEntry[] = Array.from({ length: 50 }, (_, index) => ({
      path: `deep/x${String(index)}.txt`,
      kind: "file",
    }));
    many.push({ path: "x.txt", kind: "file" }, { path: "x.txt", kind: "file" });
    const result = filterTree(many, "x", 5);
    expect(result.matched).toBe(51);
    const matches = result.rows.filter((row) => row.context !== true);
    expect(matches).toHaveLength(5);
    // The shallow name wins; then numbers sort by value.
    const kept = matches.map((row) => row.path);
    expect(kept).toContain("x.txt");
    expect(kept).toContain("deep/x2.txt");
    expect(kept).not.toContain("deep/x10.txt");
  });

  it("searches what the tree has read until the index is in", () => {
    const state = run([
      { type: "loaded", path: "", listing: listing([dir("src"), file("a.md")]) },
      { type: "loaded", path: "src", listing: listing([file("b.ts")]) },
      { type: "failed", path: "docs", message: "x" },
    ]);
    expect(loadedPaths(state)).toEqual([
      { path: "src", kind: "dir" },
      { path: "a.md", kind: "file" },
      { path: "src/b.ts", kind: "file" },
    ]);
  });

  it("walks the filtered tree without folding it", () => {
    const rows = filterTree(paths, "main").rows;
    expect(filteredStep(rows, "src/ui/MainView.tsx", "ArrowDown")).toEqual({
      kind: "focus",
      path: "src/main.rs",
    });
    // Left on an open folder goes up instead of closing it; at the top there is nowhere to go.
    expect(filteredStep(rows, "src/ui", "ArrowLeft")).toEqual({ kind: "focus", path: "src" });
    expect(filteredStep(rows, "src", "ArrowLeft")).toEqual({ kind: "none" });
    // Enter on a folder shows it in the whole tree; on a file it opens the file.
    expect(filteredStep(rows, "src/ui", "Enter")).toEqual({ kind: "reveal", path: "src/ui" });
    expect(filteredStep(rows, "docs/main.md", "Enter")).toEqual({
      kind: "open",
      path: "docs/main.md",
    });
    expect(filteredStep(rows, "src", "x")).toBeNull();
  });

  it("opens the folders on the way when a match is revealed", () => {
    expect(revealFolders("a/b/c", true)).toEqual(["a", "a/b", "a/b/c"]);
    expect(revealFolders("a/b/c.txt", false)).toEqual(["a", "a/b"]);
    expect(revealFolders("top.txt", false)).toEqual([]);
  });

  it("starts with a typed character, never with a shortcut or Space", () => {
    expect(startsFilter("a", false)).toBe(true);
    expect(startsFilter("ä", false)).toBe(true);
    expect(startsFilter(".", false)).toBe(true);
    expect(startsFilter("a", true)).toBe(false);
    expect(startsFilter(" ", false)).toBe(false);
    expect(startsFilter("ArrowDown", false)).toBe(false);
    expect(startsFilter("Enter", false)).toBe(false);
  });
});
