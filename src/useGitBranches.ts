// Git branch per working folder, refreshed every BRANCH_REFRESH_MS while the window is visible and
// at once when it comes back. Errors are swallowed: the status line then simply shows no branch.

import { useEffect, useRef, useState } from "react";

import { api } from "./api";
import { BRANCH_REFRESH_MS, parseBranches, sameBranches } from "./gitBranches";

const NO_BRANCHES: ReadonlyMap<string, string | null> = new Map();

/** `paths`: the folders to ask for (`branchPaths`); a new list is asked for right away. */
export function useGitBranches(paths: readonly string[]): ReadonlyMap<string, string | null> {
  const [branches, setBranches] = useState(NO_BRANCHES);
  const key = paths.join("\n");
  const pathsRef = useRef(paths);
  useEffect(() => {
    pathsRef.current = paths;
  });

  useEffect(() => {
    let disposed = false;
    let running = false;
    const refresh = () => {
      const current = pathsRef.current;
      if (running || document.visibilityState !== "visible") return;
      if (current.length === 0) {
        setBranches((old) => (old.size === 0 ? old : NO_BRANCHES));
        return;
      }
      running = true;
      api
        .gitBranches([...current])
        .then((value) => {
          if (disposed) return;
          const next = parseBranches(value);
          setBranches((old) => (sameBranches(old, next) ? old : next));
        })
        .catch(() => undefined)
        .finally(() => {
          running = false;
        });
    };
    refresh();
    const timer = window.setInterval(refresh, BRANCH_REFRESH_MS);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [key]);

  return branches;
}
