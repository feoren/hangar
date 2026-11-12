import { describe, expect, it } from "vitest";

import {
  appendPage,
  bodyPreview,
  COMMIT_PAGE,
  commitAge,
  commitGroups,
  isFullSha,
  parseCommitDiff,
  parseGitLog,
  readLog,
  reloadLimit,
  type CommitSummary,
  type GitLog,
} from "./gitHistory";

const SHA_A = "a".repeat(40);
const SHA_B = "b".repeat(40);
const SHA_C = "c".repeat(64);

function commit(sha: string, pushed: boolean): CommitSummary {
  return {
    sha,
    shortSha: sha.slice(0, 7),
    subject: `subject ${sha.charAt(0)}`,
    author: "Ada",
    authoredAt: 1_700_000_000,
    pushed,
  };
}

function log(commits: CommitSummary[], extra: Partial<GitLog> = {}): GitLog {
  return {
    isRepo: true,
    commits,
    upstream: "origin/main",
    aheadCount: 0,
    hasMore: false,
    ...extra,
  };
}

describe("isFullSha", () => {
  it("accepts SHA-1 and SHA-256 ids only", () => {
    expect(isFullSha(SHA_A)).toBe(true);
    expect(isFullSha(SHA_C)).toBe(true);
    expect(isFullSha("abc1234")).toBe(false);
    expect(isFullSha("A".repeat(40))).toBe(false);
    expect(isFullSha(`--${"a".repeat(38)}`)).toBe(false);
    expect(isFullSha(42)).toBe(false);
  });
});

describe("parseGitLog", () => {
  it("reads a page", () => {
    expect(
      parseGitLog({
        isRepo: true,
        commits: [
          {
            sha: SHA_A,
            shortSha: "aaaaaaa",
            subject: "feat: x",
            author: "Ada",
            authoredAt: 1_700_000_000,
            pushed: false,
          },
        ],
        upstream: "origin/main",
        aheadCount: 1,
        hasMore: true,
      }),
    ).toEqual({
      isRepo: true,
      commits: [
        {
          sha: SHA_A,
          shortSha: "aaaaaaa",
          subject: "feat: x",
          author: "Ada",
          authoredAt: 1_700_000_000,
          pushed: false,
        },
      ],
      upstream: "origin/main",
      aheadCount: 1,
      hasMore: true,
    });
  });

  it("drops commits without a full id and fills missing fields", () => {
    const parsed = parseGitLog({
      isRepo: true,
      commits: [{ sha: "abc" }, { sha: SHA_B }, null, "x"],
      upstream: null,
    });
    expect(parsed?.commits).toEqual([
      { sha: SHA_B, shortSha: "bbbbbbb", subject: "", author: "", authoredAt: 0, pushed: false },
    ]);
    expect(parsed?.upstream).toBeNull();
    expect(parsed?.aheadCount).toBe(0);
    expect(parsed?.hasMore).toBe(false);
  });

  it("rejects malformed answers", () => {
    expect(parseGitLog(null)).toBeNull();
    expect(parseGitLog([])).toBeNull();
    expect(parseGitLog({ isRepo: "yes", commits: [] })).toBeNull();
    expect(parseGitLog({ isRepo: true })).toBeNull();
  });
});

describe("parseCommitDiff", () => {
  const valid = {
    commit: {
      sha: SHA_A,
      shortSha: "aaaaaaa",
      parents: [SHA_B],
      subject: "fix: y",
      body: "Because.",
      author: "Ada",
      authoredAt: 1_700_000_000,
    },
    isRepo: true,
    files: [{ path: "src/a.ts", status: "M ", additions: 1, deletions: 2 }],
    diff: "",
    truncated: false,
  };

  it("reads the header and the diff", () => {
    const parsed = parseCommitDiff(valid);
    expect(parsed?.commit).toEqual(valid.commit);
    expect(parsed?.files).toEqual(valid.files);
    expect(parsed?.truncated).toBe(false);
  });

  it("rejects an answer without a valid header", () => {
    expect(parseCommitDiff({ ...valid, commit: undefined })).toBeNull();
    expect(parseCommitDiff({ ...valid, commit: { ...valid.commit, sha: "nothex" } })).toBeNull();
    expect(parseCommitDiff({ ...valid, commit: { ...valid.commit, parents: "x" } })).toBeNull();
    expect(parseCommitDiff({ ...valid, files: "x" })).toBeNull();
  });

  it("keeps only full parent ids", () => {
    const parsed = parseCommitDiff({
      ...valid,
      commit: { ...valid.commit, parents: [SHA_B, "zz", 3] },
    });
    expect(parsed?.commit.parents).toEqual([SHA_B]);
  });
});

describe("appendPage", () => {
  it("adds new commits and takes the newer counts", () => {
    const next = appendPage(
      log([commit(SHA_A, false)], { hasMore: true, aheadCount: 1 }),
      log([commit(SHA_B, true)], { hasMore: true, aheadCount: 2 }),
    );
    expect(next.commits.map((c) => c.sha)).toEqual([SHA_A, SHA_B]);
    expect(next.aheadCount).toBe(2);
    expect(next.hasMore).toBe(true);
  });

  it("skips commits already listed", () => {
    const next = appendPage(
      log([commit(SHA_A, false)], { hasMore: true }),
      log([commit(SHA_A, false), commit(SHA_B, true)], { hasMore: false }),
    );
    expect(next.commits.map((c) => c.sha)).toEqual([SHA_A, SHA_B]);
    expect(next.hasMore).toBe(false);
  });

  it("stops paging on a page that adds nothing", () => {
    const current = log([commit(SHA_A, false)], { hasMore: true });
    expect(appendPage(current, log([], { hasMore: true })).hasMore).toBe(false);
    expect(appendPage(current, log([commit(SHA_A, false)], { hasMore: true })).hasMore).toBe(false);
  });
});

describe("reloadLimit", () => {
  it("reads at least one page and at most what the backend serves", () => {
    expect(reloadLimit(0)).toBe(COMMIT_PAGE);
    expect(reloadLimit(60)).toBe(60);
    expect(reloadLimit(400)).toBe(100);
  });
});

describe("readLog", () => {
  /** A history of `total` commits served in pages like `git_log`, recording each request. */
  function history(total: number) {
    const shas = Array.from({ length: total }, (_, index) => index.toString(16).padStart(40, "0"));
    const calls: [number, number][] = [];
    const fetchPage = (skip: number, limit: number): Promise<unknown> => {
      calls.push([skip, limit]);
      const served = Math.min(limit, 100);
      return Promise.resolve({
        isRepo: true,
        commits: shas.slice(skip, skip + served).map((sha) => commit(sha, true)),
        upstream: null,
        aheadCount: 0,
        hasMore: skip + served < total,
      });
    };
    return { fetchPage, calls };
  }

  it("reads one page when no more is listed", async () => {
    const { fetchPage, calls } = history(500);
    const result = await readLog(fetchPage, 0);
    expect(result?.commits).toHaveLength(COMMIT_PAGE);
    expect(result?.hasMore).toBe(true);
    expect(calls).toEqual([[0, COMMIT_PAGE]]);
  });

  it("keeps every loaded commit beyond the largest page", async () => {
    const { fetchPage, calls } = history(500);
    const result = await readLog(fetchPage, 150);
    expect(result?.commits).toHaveLength(150);
    expect(result?.hasMore).toBe(true);
    expect(calls).toEqual([
      [0, 100],
      [100, 50],
    ]);
  });

  it("stops at the end of a short history", async () => {
    const { fetchPage } = history(120);
    const result = await readLog(fetchPage, 300);
    expect(result?.commits).toHaveLength(120);
    expect(result?.hasMore).toBe(false);
  });

  it("returns null for a malformed page", async () => {
    let first = true;
    const result = await readLog(() => {
      const answer: unknown = first
        ? log([commit(SHA_A, true)], { hasMore: true })
        : { isRepo: "yes" };
      first = false;
      return Promise.resolve(answer);
    }, 60);
    expect(result).toBeNull();
  });
});

describe("commitGroups", () => {
  it("puts unpushed commits first when there is an upstream", () => {
    const groups = commitGroups(
      log([commit(SHA_A, false), commit(SHA_B, true), commit(SHA_C, false)]),
    );
    expect(groups.map((group) => group.kind)).toEqual(["unpushed", "pushed"]);
    expect(groups[0]?.commits.map((c) => c.sha)).toEqual([SHA_A, SHA_C]);
  });

  it("leaves out empty groups", () => {
    expect(commitGroups(log([commit(SHA_B, true)])).map((group) => group.kind)).toEqual(["pushed"]);
    expect(commitGroups(log([]))).toEqual([]);
  });

  it("keeps one group without an upstream", () => {
    const groups = commitGroups(log([commit(SHA_A, false)], { upstream: null }));
    expect(groups).toEqual([{ kind: "all", commits: [commit(SHA_A, false)] }]);
    expect(commitGroups(log([], { upstream: null }))).toEqual([]);
  });
});

describe("commitAge", () => {
  const now = 1_700_000_000 * 1000;
  const age = (seconds: number) => commitAge(1_700_000_000 - seconds, now, "en");

  it("names the largest whole unit", () => {
    expect(age(10)).toBe("now");
    expect(age(5 * 60)).toBe("5 min. ago");
    expect(age(3 * 3600)).toBe("3 hr. ago");
    expect(age(86_400)).toBe("yesterday");
    expect(age(14 * 86_400)).toBe("2 wk. ago");
    expect(age(60 * 86_400)).toBe("2 mo. ago");
    expect(age(800 * 86_400)).toBe("2 yr. ago");
  });

  it("treats a future date as now", () => {
    expect(age(-3600)).toBe("now");
  });

  it("speaks the UI language", () => {
    expect(commitAge(1_700_000_000 - 86_400, now, "de")).toBe("gestern");
  });
});

describe("bodyPreview", () => {
  it("keeps a short body whole without trailing blank lines", () => {
    expect(bodyPreview("one\r\ntwo\n\n")).toEqual({ text: "one\ntwo", long: false });
    expect(bodyPreview("")).toEqual({ text: "", long: false });
  });

  it("cuts a long body", () => {
    const body = Array.from({ length: 9 }, (_, i) => `line ${String(i)}`).join("\n");
    const preview = bodyPreview(body);
    expect(preview.long).toBe(true);
    expect(preview.text.split("\n")).toHaveLength(6);
  });
});
