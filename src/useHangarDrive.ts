import { useEffect, useRef, useState } from "react";

import { api } from "./api";
import { MAX_DRIVE_PATHS, NO_DRIVE, parseDriveStatus, type DriveView } from "./hangarDrive";

/**
 * Whether Hangar Drive is installed and which of `folders` it holds. Asked when the folders
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
