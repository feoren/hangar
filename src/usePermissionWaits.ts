// The sessions waiting for an agent's dialog (permissionWaits.ts) in the main window: read once
// and again on every `permission-waits-changed` signal, which carries no payload.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

import { api, PERMISSION_WAITS_CHANGED } from "./api";
import { parsePermissionWaiting } from "./permissionWaits";

export function usePermissionWaits(): ReadonlySet<number> {
  const [waiting, setWaiting] = useState<ReadonlySet<number>>(() => new Set());
  // Only the answer to the newest read counts: two reads may answer out of order.
  const reads = useRef(0);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    const read = () => {
      const ticket = ++reads.current;
      api
        .permissionWaiting()
        .then((value) => {
          if (!disposed && ticket === reads.current) setWaiting(parsePermissionWaiting(value));
        })
        .catch(() => undefined);
    };
    try {
      listen(PERMISSION_WAITS_CHANGED, read)
        .then((unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        })
        .catch(() => undefined);
      read();
    } catch {
      // Outside Tauri (browser preview) there are no backend events.
    }
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  return waiting;
}
