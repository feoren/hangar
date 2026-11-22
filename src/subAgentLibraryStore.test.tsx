import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  library: vi.fn<(folders: readonly string[]) => Promise<unknown>>(),
  sync: vi.fn<(folders: readonly string[]) => Promise<null>>(),
}));

vi.mock("./api", () => ({
  api: {
    subAgentLibrary: mocks.library,
    subAgentFigures: () => Promise.resolve({}),
    syncSubAgents: mocks.sync,
  },
  errorText: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

import {
  refreshLibrary,
  resetLibraryStore,
  setLibraryFolders,
  useSubAgentLibrary,
} from "./subAgentLibraryStore";

const LISTING = { defs: [], problems: [] };

/** The folders the shown listing was read for, as the store publishes them right now. */
function shownRead(): readonly string[] | null {
  let read: readonly string[] | null = null;
  function Probe() {
    read = useSubAgentLibrary().read;
    return null;
  }
  renderToStaticMarkup(<Probe />);
  return read;
}

function shownError(): string | null {
  let error: string | null = null;
  function Probe() {
    error = useSubAgentLibrary().error;
    return null;
  }
  renderToStaticMarkup(<Probe />);
  return error;
}

describe("the folders the shown listing covers", () => {
  beforeEach(() => {
    resetLibraryStore();
    mocks.library.mockReset();
    mocks.sync.mockReset();
    mocks.sync.mockResolvedValue(null);
  });

  it("are none until the first listing arrives", () => {
    expect(shownRead()).toBeNull();
  });

  it("are the folders that listing was asked for", async () => {
    mocks.library.mockResolvedValue(LISTING);
    setLibraryFolders(["C:\\src\\a", "C:\\src\\b"]);
    await vi.waitFor(() => {
      expect(shownRead()).not.toBeNull();
    });
    expect(mocks.library).toHaveBeenLastCalledWith(["C:\\src\\a", "C:\\src\\b"]);
    expect(shownRead()).toEqual(["C:\\src\\a", "C:\\src\\b"]);
  });

  it("stop at the folder limit of 32", async () => {
    mocks.library.mockResolvedValue(LISTING);
    const many = Array.from({ length: 40 }, (_, i) => `C:\\src\\p${String(i)}`);
    setLibraryFolders(many);
    await vi.waitFor(() => {
      expect(shownRead()).not.toBeNull();
    });
    const read = shownRead();
    expect(read).toHaveLength(32);
    expect(read).not.toContain("C:\\src\\p32");
  });

  it("are none after an answer that does not parse", async () => {
    mocks.library.mockResolvedValueOnce(LISTING).mockResolvedValueOnce({ defs: "broken" });
    setLibraryFolders(["C:\\src\\a"]);
    await vi.waitFor(() => {
      expect(shownRead()).toEqual(["C:\\src\\a"]);
    });
    await refreshLibrary();
    expect(shownRead()).toBeNull();
  });

  it("stay those of the last good listing when a refresh fails", async () => {
    mocks.library.mockResolvedValueOnce(LISTING).mockRejectedValueOnce(new Error("unreadable"));
    setLibraryFolders(["C:\\src\\a"]);
    await vi.waitFor(() => {
      expect(shownRead()).toEqual(["C:\\src\\a"]);
    });
    await refreshLibrary();
    expect(shownError()).toBe("unreadable");
    expect(shownRead()).toEqual(["C:\\src\\a"]);
  });
});
