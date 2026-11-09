// Local claims in the window: tells the backend whether to watch, keeps the newest snapshot and
// hands each file-level collision to `onFresh` once per app run (claims.ts).

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

import { api, CLAIMS_CHANGED } from "./api";
import {
  EMPTY_CLAIMS,
  freshOverlaps,
  overlapKey,
  parseClaimsSnapshot,
  type ClaimsSnapshot,
  type Overlap,
} from "./claims";

export function useClaims(
  enabled: boolean,
  onFresh: (overlaps: Overlap[]) => void,
): ClaimsSnapshot {
  const [snapshot, setSnapshot] = useState<ClaimsSnapshot>(EMPTY_CLAIMS);
  const seen = useRef(new Set<string>());
  const onFreshRef = useRef(onFresh);
  useEffect(() => {
    onFreshRef.current = onFresh;
  });

  const accept = useRef((value: unknown) => {
    const parsed = parseClaimsSnapshot(value);
    if (parsed === null) return;
    setSnapshot(parsed);
    const fresh = freshOverlaps(seen.current, parsed);
    for (const overlap of fresh) seen.current.add(overlapKey(overlap));
    if (fresh.length > 0) onFreshRef.current(fresh);
  });

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    try {
      listen(CLAIMS_CHANGED, (event) => {
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
    api
      .configureClaims(enabled)
      .then(() => (enabled ? api.claimsSnapshot() : EMPTY_CLAIMS))
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
