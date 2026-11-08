// Shared clock for relative times ("working for 3 min"): one timer for the whole sidebar, so the
// times move on without any render per output chunk and without re-rendering the terminals.

import { useEffect, useState } from "react";

/** Current time (Date.now), refreshed every `intervalMs` and when the window becomes visible. */
export function useClock(intervalMs: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const tick = () => {
      if (document.visibilityState === "visible") setNow(Date.now());
    };
    const timer = window.setInterval(tick, intervalMs);
    document.addEventListener("visibilitychange", tick);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  }, [intervalMs]);
  return now;
}
