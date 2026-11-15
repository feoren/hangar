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

/**
 * `guard`: the command guard's setting, sent whenever it changes; the backend applies it to
 * running sessions at once, so no start waits for it. `null` for either: not known yet (the
 * switches load from the backend, mainStore.ts); nothing is sent until it is, and the restore
 * waits for the gate's first setting (`gateReady`).
 */
export function useGate(
  enabled: boolean | null,
  guard: boolean | null,
  onFresh: (decisions: PendingDecision[]) => void,
): GateSnapshot {
  const [snapshot, setSnapshot] = useState<GateSnapshot>(EMPTY_GATE);
  const seen = useRef(new Set<number>());
  const onFreshRef = useRef(onFresh);
  useEffect(() => {
    onFreshRef.current = onFresh;
  });

  // Only the answer to the newest read counts: two reads may answer out of order.
  const reads = useRef(0);
  const read = useRef(() => {
    const ticket = ++reads.current;
    return api.gateSnapshot().then((value) => {
      if (ticket === reads.current) accept.current(value);
    });
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
      // The event carries no snapshot (any window could listen to it); the main window reads it.
      listen(GATE_CHANGED, () => {
        read.current().catch(() => undefined);
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
    if (enabled === null) return;
    let current = true;
    // Turned off, the backend allows every waiting decision; its snapshot then has none left.
    setup
      .configure(enabled)
      .then(() => (current ? read.current() : undefined))
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [enabled]);

  useEffect(() => {
    if (guard === null) return;
    api.configureCommandGuard(guard).catch(() => undefined);
  }, [guard]);

  return snapshot;
}
