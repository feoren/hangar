import { useEffect, useState } from "react";

import { rowPlace, waitingHints, type RowPlace, type WaitingHints } from "./waitingHint";

const NO_HINTS: WaitingHints = { above: null, below: null };

/**
 * Waiting sessions scrolled out of a list's view: a calm hint at that edge (waitingHint.ts). One
 * IntersectionObserver per list watches only the waiting rows, so nothing runs per output chunk.
 */
export function useWaitingEdges(
  list: React.RefObject<HTMLUListElement | null>,
  /** Session ids of this list, in its order. */
  order: readonly number[],
  /** Waiting session ids in this list. */
  waiting: readonly number[],
  /** Off while the list is not shown (collapsed sidebar or group). */
  enabled: boolean,
) {
  const [places, setPlaces] = useState<ReadonlyMap<number, RowPlace>>(new Map());
  const waitingKey = waiting.join(",");
  const rowKey = order.join(",");
  useEffect(() => {
    const element = list.current;
    if (!enabled || !element || waitingKey === "" || typeof IntersectionObserver === "undefined") {
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        setPlaces((current) => {
          const next = new Map(current);
          for (const entry of entries) {
            const id = Number(entry.target.getAttribute("data-session-row"));
            const view = entry.rootBounds ?? element.getBoundingClientRect();
            next.set(id, rowPlace(entry.boundingClientRect, view));
          }
          return next;
        });
      },
      { root: element, threshold: [0, 0.5, 1] },
    );
    for (const id of waitingKey.split(",")) {
      const row = element.querySelector(`[data-session-row="${id}"]`);
      if (row) observer.observe(row);
    }
    return () => {
      observer.disconnect();
    };
  }, [list, enabled, waitingKey, rowKey]);
  // A fresh observer reports every watched row at once, so older places never linger visibly.
  return enabled ? waitingHints(order, new Set(waiting), places) : NO_HINTS;
}
