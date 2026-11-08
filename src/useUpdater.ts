// Checks for signed updates (latest.json, see tools/release.py) and installs them on click.

import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useCallback, useEffect, useState } from "react";

import { errorText } from "./api";
import { t } from "./i18n";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking"; manual: boolean }
  | { kind: "current"; version: string }
  | { kind: "available"; update: Update }
  | { kind: "installing"; update: Update; percent: number | null }
  | { kind: "error"; message: string };

const FIRST_CHECK_DELAY_MS = 5_000;
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/** `autoCheck`: check regularly in the background (a setting). Manual checks always work. */
export function useUpdater(currentVersion: string | null, autoCheck: boolean) {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  const runCheck = useCallback(
    async (manual: boolean) => {
      setState((current) =>
        current.kind === "installing" ? current : { kind: "checking", manual },
      );
      try {
        const update = await check();
        setState((current) => {
          if (current.kind === "installing") return current;
          if (update) return { kind: "available", update };
          // Automatic checks that find nothing stay silent.
          return manual ? { kind: "current", version: currentVersion ?? "" } : { kind: "idle" };
        });
      } catch (error) {
        // Offline or server unreachable: only report it if the user asked.
        setState(manual ? { kind: "error", message: errorText(error) } : { kind: "idle" });
      }
    },
    [currentVersion],
  );

  useEffect(() => {
    // In development mode there is no update server; checking manually still works.
    if (import.meta.env.DEV || !autoCheck) return;
    const first = window.setTimeout(() => void runCheck(false), FIRST_CHECK_DELAY_MS);
    const interval = window.setInterval(() => void runCheck(false), CHECK_INTERVAL_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(interval);
    };
  }, [runCheck, autoCheck]);

  const install = useCallback(async (update: Update) => {
    setState({ kind: "installing", update, percent: null });
    let total = 0;
    let done = 0;
    try {
      // On Windows the app quits by itself for the installer (installMode "passive").
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          done += event.data.chunkLength;
        } else {
          done = total;
        }
        const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : null;
        setState({ kind: "installing", update, percent });
      });
      await relaunch();
    } catch (error) {
      setState({ kind: "error", message: t("update.failed", { error: errorText(error) }) });
    }
  }, []);

  const dismiss = useCallback(() => {
    setState({ kind: "idle" });
  }, []);

  return { state, check: runCheck, install, dismiss };
}
