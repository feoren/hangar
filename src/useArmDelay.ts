// A short pause before the buttons of something that just appeared take clicks. A card that
// pops up under the mouse (a gate decision, a new one replacing the last) must not take a click
// that was meant for what was there a moment before: "Allow once" would then answer a question
// nobody read.

import { useCallback, useLayoutEffect, useState } from "react";

/** How long buttons ignore clicks after they appear or their question changes. */
export const ARM_DELAY_MS = 600;

/** The clock behind the pause; pure, so it can be tested with fake timers. */
export class ArmClock {
  private start: number;

  constructor(
    private readonly now: () => number,
    private readonly delay: number = ARM_DELAY_MS,
  ) {
    this.start = now();
  }

  /** What the buttons answer changed: the pause begins again. */
  restart(): void {
    this.start = this.now();
  }

  /** Milliseconds until clicks count, 0 once they do. */
  remaining(): number {
    return Math.max(0, this.delay - (this.now() - this.start));
  }

  ready(): boolean {
    return this.remaining() === 0;
  }

  /** `action`, run only once the pause is over; earlier calls are dropped. */
  guard<A extends unknown[]>(action: (...args: A) => void): (...args: A) => void {
    return (...args) => {
      if (this.ready()) action(...args);
    };
  }
}

/**
 * Arms buttons `delay` ms after mount and again after every change of `key` (the question they
 * answer). `armed` is for the look; `guard` wraps an action so a click before then does nothing,
 * whatever React has rendered yet.
 */
export function useArmDelay(
  key: unknown,
  delay: number = ARM_DELAY_MS,
): {
  armed: boolean;
  guard: <A extends unknown[]>(action: (...args: A) => void) => (...args: A) => void;
} {
  const [clock] = useState(() => new ArmClock(() => performance.now(), delay));
  const [armedFor, setArmedFor] = useState<{ key: unknown } | null>(null);
  // Layout effect: the pause restarts before the browser paints the new question, so no click
  // can land between the new card on screen and the restart.
  useLayoutEffect(() => {
    clock.restart();
    const timer = window.setTimeout(() => {
      setArmedFor({ key });
    }, clock.remaining());
    return () => {
      window.clearTimeout(timer);
    };
  }, [clock, key]);
  const guard = useCallback(
    <A extends unknown[]>(action: (...args: A) => void) => clock.guard(action),
    [clock],
  );
  return { armed: armedFor !== null && armedFor.key === key, guard };
}
