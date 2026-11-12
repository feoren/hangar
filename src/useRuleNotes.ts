// The short note a remembered answer of the gate leaves on its session (gate.ts): the backend
// answered an edit by a rule instead of asking, and the person should still see that it happened.

import { useEffect, useRef, useState } from "react";

import { freshRuleAnswers, RULE_NOTE_MS, type RuleAnswer } from "./gate";

/** The newest note per session, each for `RULE_NOTE_MS`. */
export function useRuleNotes(log: readonly RuleAnswer[]): ReadonlyMap<number, RuleAnswer> {
  const [notes, setNotes] = useState<ReadonlyMap<number, RuleAnswer>>(new Map());
  const lastSeen = useRef(0);
  // Timers outlive the log that started them (a newer log must not cancel an older note's end).
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending) clearTimeout(timer);
      pending.clear();
    };
  }, []);

  useEffect(() => {
    const fresh = freshRuleAnswers(lastSeen.current, log, Date.now());
    const newest = log[0];
    if (newest !== undefined) lastSeen.current = Math.max(lastSeen.current, newest.id);
    if (fresh.length === 0) return;
    setNotes((current) => {
      const next = new Map(current);
      for (const entry of fresh) next.set(entry.sessionId, entry);
      return next;
    });
    for (const entry of fresh) {
      const timer = setTimeout(() => {
        timers.current.delete(timer);
        setNotes((current) => {
          if (current.get(entry.sessionId)?.id !== entry.id) return current;
          const next = new Map(current);
          next.delete(entry.sessionId);
          return next;
        });
      }, RULE_NOTE_MS);
      timers.current.add(timer);
    }
  }, [log]);

  return notes;
}
