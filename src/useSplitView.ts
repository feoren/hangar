// Split view state for App: which session sits in which pane, mode changes, maximize and
// "show exactly these". The pure rules live in split.ts; this hook only holds the slots and wires
// them to the layout.

import { useState, type Dispatch, type SetStateAction } from "react";

import type { Layout } from "./layout";
import type { SessionView } from "./sessions";
import {
  EMPTY_SLOTS,
  reconcileSlots,
  showSessions,
  slotOf,
  type SplitMode,
  type SplitSlots,
} from "./split";
import type { PaneSplit } from "./TerminalPane";

interface Options {
  mode: SplitMode;
  sessions: readonly SessionView[];
  activeId: number | null;
  setLayout: Dispatch<SetStateAction<Layout>>;
  activate: (id: number) => void;
  focusTerminal: () => void;
}

/** What a pane header shows besides its position (badge, attention state, status text). */
export type PaneLook = Omit<PaneSplit, "mode" | "slot" | "session">;

export interface SplitView {
  mode: SplitMode;
  split: SplitSlots;
  setSplitMode: (mode: SplitMode) => void;
  /** Double click on a pane header: show only this session (View menu brings the split back). */
  maximize: (id: number) => void;
  /** Shows exactly these sessions (after "Send to several"), the first one focused. */
  showOnly: (ids: readonly number[]) => void;
  /** Header and position of a visible pane; `null` in single view or when it is hidden. */
  paneSplit: (session: SessionView, look: () => PaneLook) => PaneSplit | null;
}

export function useSplitView(options: Options): SplitView {
  const { mode, sessions, activeId, setLayout, activate, focusTerminal } = options;
  const [slots, setSlots] = useState<SplitSlots>(EMPTY_SLOTS);

  // Panes follow the sessions: closed ones leave, the active one is always shown and focused.
  // Adjusted during render, so no frame shows a pane without its session.
  const split = reconcileSlots(
    slots,
    mode,
    sessions.map((session) => session.id),
    activeId,
  );
  if (split !== slots) setSlots(split);

  const setSplitMode = (next: SplitMode) => {
    setLayout((current) =>
      current.splitMode === next ? current : { ...current, splitMode: next },
    );
    focusTerminal();
  };

  const maximize = (id: number) => {
    setLayout((current) => ({ ...current, splitMode: "single" }));
    activate(id);
    focusTerminal();
  };

  const showOnly = (ids: readonly number[]) => {
    const next = showSessions(ids, mode);
    setLayout((current) => ({ ...current, splitMode: next.mode }));
    setSlots(next.split);
    const first = ids[0];
    if (first !== undefined) activate(first);
    focusTerminal();
  };

  const paneSplit = (session: SessionView, look: () => PaneLook): PaneSplit | null => {
    if (mode === "single") return null;
    const slot = slotOf(split, session.id);
    if (slot === null) return null;
    return { mode, slot, session, ...look() };
  };

  return { mode, split, setSplitMode, maximize, showOnly, paneSplit };
}
