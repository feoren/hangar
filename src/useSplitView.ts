// Split view state for App: which session sits in which pane, layout changes, placing a session
// in a zone, maximize and "show exactly these". The pure rules live in split.ts and
// snapLayouts.ts; this hook only holds the slots and wires them to the layout.

import { useState, type Dispatch, type SetStateAction } from "react";

import type { Layout } from "./layout";
import type { SessionView } from "./sessions";
import type { LayoutId } from "./snapLayouts";
import {
  EMPTY_SLOTS,
  placeSession,
  reconcileSlots,
  showSessions,
  slotOf,
  type SplitSlots,
} from "./split";
import type { PaneSplit } from "./TerminalPane";

interface Options {
  layout: LayoutId;
  sessions: readonly SessionView[];
  activeId: number | null;
  setLayout: Dispatch<SetStateAction<Layout>>;
  activate: (id: number) => void;
  focusTerminal: () => void;
}

/** What a pane header shows besides its position (badge, attention state, status text). */
export type PaneLook = Omit<PaneSplit, "layout" | "slot" | "session">;

export interface SplitView {
  layout: LayoutId;
  split: SplitSlots;
  setSnapLayout: (layout: LayoutId) => void;
  /**
   * Shows this session in one zone of a layout and makes it active (drop from the sidebar,
   * layout picker); the other panes keep their sessions where they can.
   */
  place: (id: number, layout: LayoutId, zone: number) => void;
  /** Double click on a pane header: show only this session (View menu brings the split back). */
  maximize: (id: number) => void;
  /** Shows exactly these sessions (after "Send to several"), the first one focused. */
  showOnly: (ids: readonly number[]) => void;
  /** Header and position of a visible pane; `null` in single view or when it is hidden. */
  paneSplit: (session: SessionView, look: () => PaneLook) => PaneSplit | null;
}

export function useSplitView(options: Options): SplitView {
  const { layout, sessions, activeId, setLayout, activate, focusTerminal } = options;
  const [slots, setSlots] = useState<SplitSlots>(EMPTY_SLOTS);
  const sessionIds = sessions.map((session) => session.id);

  // Panes follow the sessions: closed ones leave, the active one is always shown and focused.
  // Adjusted during render, so no frame shows a pane without its session.
  const split = reconcileSlots(slots, layout, sessionIds, activeId);
  if (split !== slots) setSlots(split);

  const setSnapLayout = (next: LayoutId) => {
    setLayout((current) =>
      current.snapLayout === next ? current : { ...current, snapLayout: next },
    );
    focusTerminal();
  };

  const place = (id: number, next: LayoutId, zone: number) => {
    if (!sessionIds.includes(id)) return;
    setLayout((current) =>
      current.snapLayout === next ? current : { ...current, snapLayout: next },
    );
    setSlots(placeSession(split, next, zone, id, sessionIds));
    activate(id);
    focusTerminal();
  };

  const maximize = (id: number) => {
    setLayout((current) => ({ ...current, snapLayout: "single" }));
    activate(id);
    focusTerminal();
  };

  const showOnly = (ids: readonly number[]) => {
    const next = showSessions(ids, layout);
    setLayout((current) => ({ ...current, snapLayout: next.layout }));
    setSlots(next.split);
    const first = ids[0];
    if (first !== undefined) activate(first);
    focusTerminal();
  };

  const paneSplit = (session: SessionView, look: () => PaneLook): PaneSplit | null => {
    if (layout === "single") return null;
    const slot = slotOf(split, session.id);
    if (slot === null) return null;
    return { layout, slot, session, ...look() };
  };

  return { layout, split, setSnapLayout, place, maximize, showOnly, paneSplit };
}
