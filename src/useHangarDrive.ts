import { useEffect, useRef, useState } from "react";

import { api } from "./api";
import { MAX_DRIVE_PATHS, NO_DRIVE, parseDriveStatus, type DriveView } from "./hangarDrive";
import { MAX_PLACE_PATHS, parseRepoPlaces } from "./projectCards";

/**
 * Whether Hangar Drive can take a folder and which of `folders` it holds. Asked when the folders
 * change and whenever the window comes back to the front: after "Back up", the person confirms
 * in Hangar Drive's own window and returns, and the mark appears then. No polling and no file
 * watcher: the list only changes through Hangar Drive, which the person has to switch to.
 */
export function useHangarDrive(folders: readonly string[]): DriveView {
  const [view, setView] = useState<DriveView>(NO_DRIVE);
  const asked = folders.slice(0, MAX_DRIVE_PATHS);
  const key = asked.join("\n");
  const askedRef = useRef(asked);
  useEffect(() => {
    askedRef.current = asked;
  });

  useEffect(() => {
    let alive = true;
    let latest = 0;
    const load = () => {
      const paths = [...askedRef.current];
      const call = ++latest;
      api.hangarDriveStatus(paths).then(
        (value) => {
          if (alive && call === latest) setView(parseDriveStatus(paths, value));
        },
        () => {
          if (alive && call === latest) setView(NO_DRIVE);
        },
      );
    };
    const visible = () => {
      if (document.visibilityState === "visible") load();
    };
    load();
    window.addEventListener("focus", load);
    document.addEventListener("visibilitychange", visible);
    return () => {
      alive = false;
      window.removeEventListener("focus", load);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [key]);

  return view;
}

const NO_ROOTS: ReadonlyMap<string, string | null> = new Map();

/**
 * The repository main folder of each of `folders` (`null` outside a repository or when git
 * failed), asked when the folders change, so the session menu knows which folder "Back up" would
 * hand over before it offers the entry (`handOverFolder`). A folder git has not answered for yet
 * is missing. Reads only `.git` files in the backend, no process.
 */
export function useRepoRoots(folders: readonly string[]): ReadonlyMap<string, string | null> {
  const [roots, setRoots] = useState(NO_ROOTS);
  const key = folders.join("\n");
  const foldersRef = useRef(folders);
  useEffect(() => {
    foldersRef.current = folders;
  });

  useEffect(() => {
    const asked = [...foldersRef.current];
    if (asked.length === 0) {
      setRoots(NO_ROOTS);
      return;
    }
    let alive = true;
    const chunks: string[][] = [];
    for (let start = 0; start < asked.length; start += MAX_PLACE_PATHS) {
      chunks.push(asked.slice(start, start + MAX_PLACE_PATHS));
    }
    void Promise.all(
      chunks.map((chunk) =>
        api.gitRepoPlaces(chunk).then(parseRepoPlaces, () => new Map<string, null>()),
      ),
    ).then((answers) => {
      if (!alive) return;
      const next = new Map<string, string | null>();
      for (const [index, chunk] of chunks.entries()) {
        const places = answers[index];
        for (const folder of chunk) next.set(folder, places?.get(folder)?.root ?? null);
      }
      setRoots(next);
    });
    return () => {
      alive = false;
    };
  }, [key]);

  return roots;
}
