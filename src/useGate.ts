// The gate in the window: tells the backend whether new Claude Code sessions get the hook, keeps
// the newest snapshot of pending decisions and hands each new decision to `onFresh` once per app
// run (gate.ts), for the chime and the desktop notification.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

import { api, GATE_CHANGED } from "./api";
import {
  EMPTY_GATE,
  freshDecisions,
  gateSetup,
  parseGateSnapshot,
  type GateSnapshot,
  type PendingDecision,
} from "./gate";

/** One per app run: the restore waits for the first setting to reach the backend. */
const setup = gateSetup((enabled) => api.configureGate(enabled));

/** Settles once the backend heard the gate setting for the first time (gate.ts `gateSetup`). */
export const gateReady: Promise<void> = setup.ready;

export function useGate(
  enabled: boolean,
  onFresh: (decisions: PendingDecision[]) => void,
): GateSnapshot {
  const [snapshot, setSnapshot] = useState<GateSnapshot>(EMPTY_GATE);
  const seen = useRef(new Set<number>());
  const onFreshRef = useRef(onFresh);
  useEffect(() => {
    onFreshRef.current = onFresh;
  });

  const accept = useRef((value: unknown) => {
    const parsed = parseGateSnapshot(value);
    if (parsed === null) return;
    setSnapshot(parsed);
    const fresh = freshDecisions(seen.current, parsed);
    for (const decision of fresh) seen.current.add(decision.id);
    if (fresh.length > 0) onFreshRef.current(fresh);
  });

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    try {
      listen(GATE_CHANGED, (event) => {
        accept.current(event.payload);
      })
        .then((unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        })
        .catch(() => undefined);
    } catch {
      // Outside Tauri (browser preview) there are no backend events.
    }
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    let current = true;
    // Turned off, the backend allows every waiting decision; its snapshot then has none left.
    setup
      .configure(enabled)
      .then(() => api.gateSnapshot())
      .then((value) => {
        if (current) accept.current(value);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [enabled]);

  return snapshot;
}
