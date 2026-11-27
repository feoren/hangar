// The programs in "New session". Each row is one start button: a click starts a session with the
// program's default. A row with models to choose from (ADR 0022) opens them in a flyout beside
// it once the pointer rests on it, or with Right arrow; a click there starts with that model.
// Dragging a row with the mouse (or Alt+Up and Alt+Down on it) changes the order. No
// pill: a small quiet arrow at the row's end says that it has models, and the row keeps its hover
// look while its flyout is open. Touch screens have
// no hover, so there a tap simply starts. Anything below a row (an update offer) comes in as
// children and wraps onto its own line.

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";

import type { SessionModel } from "./api";
import { Badge } from "./Badge";
import { useT } from "./i18n";
import { ChevronIcon } from "./icons";
import type { ModelSection } from "./localLlm";
import { headingFor, type PointerPoint } from "./menuNav";
import { rowMoveStep } from "./shortcuts";
import { flyoutPlace, ModelFlyout, type FlyoutPlace } from "./ModelMenu";
import type { BadgeLook } from "./programPrefs";

/** How long the pointer rests on a row before its flyout opens: passing over it does nothing. */
export const FLYOUT_OPEN_DELAY_MS = 150;
/**
 * How long an open flyout waits after the pointer left its row (on the way into the flyout), and
 * how long another row waits while the pointer crosses it heading for the flyout.
 */
export const FLYOUT_GRACE_MS = 300;
/** How far the pointer moves with the button down before a press becomes a drag. */
export const DRAG_THRESHOLD_PX = 5;

/** A row being dragged: where it came from, where it would land, how far it moved. */
interface Drag {
  key: string;
  /** The row ids in their shown order when the drag began. */
  ids: readonly string[];
  from: number;
  to: number;
  /** Pointer movement since the press, in pixels. */
  dy: number;
  /** Height of the dragged row: how far the others step aside. */
  height: number;
}

interface OpenFlyout {
  key: string;
  /** Opened with the keyboard: the first model has the focus. */
  focus: boolean;
  /** Measured when it opens. */
  place: FlyoutPlace;
}

interface Pending {
  kind: "open" | "switch" | "close";
  key: string | null;
  id: number;
}

/** "restore": back to where the focus was before the flyout took it, if it did. */
type FocusAfter = "restore" | "none" | HTMLElement;

interface FlyoutControl {
  open: OpenFlyout | null;
  register: (key: string, row: HTMLLIElement | null) => void;
  /**
   * The pointer moved over a row or its flyout (`onRow`: over the row's start button). `place`
   * measures where the row's flyout would go.
   */
  move: (
    key: string,
    hasModels: boolean,
    onRow: boolean,
    point: PointerPoint,
    place: () => FlyoutPlace,
  ) => void;
  leave: () => void;
  /** Right arrow on a row. */
  show: (key: string, place: FlyoutPlace) => void;
  close: (focus: FocusAfter) => void;
  /** The primary button went down on a row: perhaps the start of a drag. */
  press: (key: string, event: ReactPointerEvent<HTMLElement>) => void;
  /** A click that ended a drag must not start the program. */
  swallowClick: (key: string) => boolean;
  /** Alt+Up or Alt+Down on a row. */
  step: (key: string, delta: -1 | 1) => void;
  drag: Drag | null;
}

const FlyoutContext = createContext<FlyoutControl | null>(null);

/**
 * The list of program rows. It owns which flyout is open, so there is only ever one, and the
 * pointer can move from one row's flyout to the next row's without waiting again.
 */
export function ProgramList(props: {
  children: ReactNode;
  /** A row was moved to `toIndex` among `shown` (the ids in their shown order). */
  onMove?: (id: string, toIndex: number, shown: readonly string[]) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState<OpenFlyout | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const onMove = useRef(props.onMove);
  const translate = useRef(t);
  useEffect(() => {
    onMove.current = props.onMove;
    translate.current = t;
  });
  const dragRef = useRef<Drag | null>(null);
  /** The row whose next click is the end of a drag. */
  const swallow = useRef<string | null>(null);
  const openRef = useRef<OpenFlyout | null>(null);
  const rows = useRef(new Map<string, HTMLLIElement>());
  const pending = useRef<Pending | null>(null);
  const lastPoint = useRef<PointerPoint | null>(null);
  /** Where the focus was when the first flyout opened. */
  const returnTo = useRef<HTMLElement | null>(null);

  const control = useMemo((): Omit<FlyoutControl, "open" | "drag"> => {
    const flyoutOf = (key: string) => rows.current.get(key)?.querySelector(".model-menu") ?? null;

    const apply = (next: OpenFlyout | null, focus: FocusAfter = "restore") => {
      const prev = openRef.current;
      if (prev !== null && prev.key !== next?.key) {
        const active = document.activeElement;
        const inside = active instanceof Node && flyoutOf(prev.key)?.contains(active) === true;
        const target = focus === "restore" ? (inside ? returnTo.current : null) : focus;
        if (target instanceof HTMLElement && target.isConnected) target.focus();
      }
      if (prev === null && next !== null) {
        const active = document.activeElement;
        returnTo.current = active instanceof HTMLElement ? active : null;
      }
      openRef.current = next;
      setOpen(next);
    };

    /** The rows in their shown order. */
    const shownRows = () => [
      ...(list.current?.querySelectorAll<HTMLLIElement>(":scope > li[data-program]") ?? []),
    ];
    const nameOf = (row: HTMLLIElement | undefined, key: string) =>
      row?.querySelector(".profile-name")?.textContent ?? key;
    /** Saves the move and says it for screen readers. */
    const moved = (key: string, name: string, to: number, ids: readonly string[]) => {
      onMove.current?.(key, to, ids);
      setAnnouncement(
        translate.current("settings.moved", { name, position: to + 1, count: ids.length }),
      );
    };

    const cancel = () => {
      if (pending.current !== null) window.clearTimeout(pending.current.id);
      pending.current = null;
    };

    const schedule = (kind: Pending["kind"], key: string | null, ms: number, run: () => void) => {
      cancel();
      const id = window.setTimeout(() => {
        pending.current = null;
        run();
      }, ms);
      pending.current = { kind, key, id };
    };

    /** The pointer is on its way into the open flyout. */
    const heading = (from: PointerPoint | null, to: PointerPoint) => {
      const current = openRef.current;
      const rect = current === null ? undefined : flyoutOf(current.key)?.getBoundingClientRect();
      if (from === null || rect === undefined) return false;
      const near = rect.left >= from.x ? rect.left : rect.right;
      return headingFor(from, to, { x: near, top: rect.top, bottom: rect.bottom });
    };

    const switchTo = (key: string, hasModels: boolean, place: () => FlyoutPlace) => {
      cancel();
      apply(hasModels ? { key, focus: false, place: place() } : null);
    };

    return {
      register: (key, row) => {
        if (row) rows.current.set(key, row);
        else rows.current.delete(key);
      },
      move: (key, hasModels, onRow, point, place) => {
        // While a row is dragged no flyout opens or switches.
        if (dragRef.current !== null) return;
        const from = lastPoint.current;
        lastPoint.current = point;
        const current = openRef.current;
        if (current?.key === key) {
          // Back on the open row or in its flyout: nothing closes or switches.
          if (pending.current !== null) cancel();
          return;
        }
        if (current === null) {
          if (!hasModels || !onRow) return;
          if (pending.current?.kind === "open" && pending.current.key === key) return;
          schedule("open", key, FLYOUT_OPEN_DELAY_MS, () => {
            apply({ key, focus: false, place: place() });
          });
          return;
        }
        // Another row's flyout is open: this row takes over at once, unless the pointer is
        // only crossing it on the way into that flyout.
        const waiting = pending.current?.kind === "switch" && pending.current.key === key;
        if (heading(from, point)) {
          if (!waiting) {
            schedule("switch", key, FLYOUT_GRACE_MS, () => {
              switchTo(key, hasModels, place);
            });
          }
          return;
        }
        switchTo(key, hasModels, place);
      },
      leave: () => {
        cancel();
        if (openRef.current === null) return;
        schedule("close", null, FLYOUT_GRACE_MS, () => {
          apply(null);
        });
      },
      show: (key, place) => {
        cancel();
        apply({ key, focus: true, place });
      },
      close: (focus) => {
        cancel();
        apply(null, focus);
      },
      press: (key, event) => {
        if (event.button !== 0 || event.pointerType === "touch" || onMove.current === undefined) {
          return;
        }
        const rows = shownRows();
        const ids = rows.map((li) => li.dataset.program ?? "");
        const from = ids.indexOf(key);
        if (from < 0) return;
        const mids = rows.map((li) => {
          const rect = li.getBoundingClientRect();
          return rect.top + rect.height / 2;
        });
        const height = rows[from]?.getBoundingClientRect().height ?? 0;
        const name = nameOf(rows[from], key);
        const startY = event.clientY;
        const pointer = event.pointerId;
        let started = false;
        const end = (commit: boolean) => {
          document.removeEventListener("pointermove", onPointerMove, true);
          document.removeEventListener("pointerup", onPointerUp, true);
          document.removeEventListener("pointercancel", onPointerCancel, true);
          document.removeEventListener("keydown", onKeyDown, true);
          const done = dragRef.current;
          dragRef.current = null;
          setDrag(null);
          if (done === null) return;
          // The click that follows the release belongs to the drag; it is dropped once.
          swallow.current = key;
          window.setTimeout(() => {
            if (swallow.current === key) swallow.current = null;
          }, 0);
          if (commit && done.to !== done.from) moved(key, name, done.to, done.ids);
        };
        const onPointerMove = (move: PointerEvent) => {
          if (move.pointerId !== pointer) return;
          const dy = move.clientY - startY;
          if (!started) {
            if (Math.abs(dy) < DRAG_THRESHOLD_PX) return;
            started = true;
            cancel();
            apply(null, "none");
          }
          move.preventDefault();
          // Where the row would land: after every other row whose middle lies above the pointer.
          const to = mids.filter((mid, index) => index !== from && mid < move.clientY).length;
          const next = { key, ids, from, to, dy, height };
          dragRef.current = next;
          setDrag(next);
        };
        const onPointerUp = (up: PointerEvent) => {
          if (up.pointerId === pointer) end(true);
        };
        const onPointerCancel = (gone: PointerEvent) => {
          if (gone.pointerId === pointer) end(false);
        };
        const onKeyDown = (down: KeyboardEvent) => {
          if (down.key === "Escape" && dragRef.current !== null) {
            down.preventDefault();
            down.stopPropagation();
            end(false);
          }
        };
        document.addEventListener("pointermove", onPointerMove, true);
        document.addEventListener("pointerup", onPointerUp, true);
        document.addEventListener("pointercancel", onPointerCancel, true);
        document.addEventListener("keydown", onKeyDown, true);
      },
      swallowClick: (key) => {
        if (swallow.current !== key) return false;
        swallow.current = null;
        return true;
      },
      step: (key, delta) => {
        if (onMove.current === undefined) return;
        const rows = shownRows();
        const ids = rows.map((li) => li.dataset.program ?? "");
        const from = ids.indexOf(key);
        const to = from + delta;
        if (from < 0 || to < 0 || to >= ids.length) return;
        cancel();
        apply(null, "none");
        moved(key, nameOf(rows[from], key), to, ids);
      },
    };
  }, []);

  useEffect(
    () => () => {
      if (pending.current !== null) window.clearTimeout(pending.current.id);
    },
    [],
  );

  // The flyout is fixed to the window and placed when it opens: once the rows scroll under it, it
  // no longer sits beside its row, so it closes. Only a scroll that moves the list counts; a
  // terminal printing output or the flyout's own list scrolling does not.
  const list = useRef<HTMLUListElement>(null);
  const isOpen = open !== null;
  useEffect(() => {
    if (!isOpen) return;
    const onScroll = (event: Event) => {
      const target = event.target;
      const moved = target instanceof Document ? target.documentElement : target;
      if (moved instanceof Node && list.current !== null && moved.contains(list.current)) {
        control.close("restore");
      }
    };
    document.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("scroll", onScroll, true);
    };
  }, [isOpen, control]);

  const value = useMemo(() => ({ ...control, open, drag }), [control, open, drag]);
  return (
    <FlyoutContext.Provider value={value}>
      <ul ref={list} className={drag === null ? "profile-list" : "profile-list sorting"}>
        {props.children}
      </ul>
      <span className="sr-only" role="status">
        {announcement}
      </span>
    </FlyoutContext.Provider>
  );
}

export function ProgramRow(props: {
  /** Unique within the list: which row a flyout belongs to. */
  id: string;
  name: string;
  /** The command line, shown in the tooltip for those who want to know what runs. */
  commandLine: string;
  badge: BadgeLook;
  /** The installed version, or null to leave it out. */
  version: string | null;
  updateAvailable: boolean;
  /** Empty: the program offers no model choice and the row opens no flyout. */
  models: readonly ModelSection[];
  onStart: (model?: SessionModel) => void;
  /** A model was picked from the flyout: the caller decides where the focus goes. */
  onMenuClose: () => void;
  children?: ReactNode;
}) {
  const t = useT();
  const control = useContext(FlyoutContext);
  const row = useRef<HTMLLIElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const label = t("sidebar.startProgram", { program: props.name });
  const drag = control?.drag ?? null;
  const dragged = drag?.key === props.id;
  // While another row is dragged, the rows between its old and new place step aside.
  const shift = (() => {
    if (drag === null) return 0;
    if (dragged) return drag.dy;
    const index = drag.ids.indexOf(props.id);
    if (drag.from < drag.to && index > drag.from && index <= drag.to) return -drag.height;
    if (drag.to < drag.from && index >= drag.to && index < drag.from) return drag.height;
    return 0;
  })();
  const hasModels = control !== null && props.models.length > 0;
  const open = hasModels && control.open?.key === props.id ? control.open : null;

  useEffect(() => {
    if (control === null) return;
    control.register(props.id, row.current);
    return () => {
      control.register(props.id, null);
    };
  }, [control, props.id]);

  const place = (): FlyoutPlace => {
    const li = row.current?.getBoundingClientRect();
    const own = button.current?.getBoundingClientRect();
    return li === undefined || own === undefined
      ? { x: 0, y: 0, flipAbove: 0 }
      : flyoutPlace({ right: li.right, top: own.top, bottom: own.bottom });
  };

  return (
    <li
      ref={row}
      className={dragged ? "profile-row dragging" : "profile-row"}
      data-program={props.id}
      style={shift === 0 ? undefined : { transform: `translateY(${String(shift)}px)` }}
      onPointerMove={(event) => {
        if (control === null || event.pointerType === "touch") return;
        const onRow = event.target instanceof Node && button.current?.contains(event.target);
        control.move(
          props.id,
          hasModels,
          onRow === true,
          { x: event.clientX, y: event.clientY },
          place,
        );
      }}
      onPointerLeave={(event) => {
        if (control === null || event.pointerType === "touch") return;
        control.leave();
      }}
    >
      <button
        ref={button}
        type="button"
        className="profile"
        aria-label={label}
        title={`${label}\n${props.commandLine}\n${t("sidebar.dragToSort")}`}
        aria-haspopup={hasModels ? "menu" : undefined}
        aria-expanded={hasModels ? open !== null : undefined}
        onPointerDown={(event) => {
          control?.press(props.id, event);
        }}
        onClick={() => {
          if (control?.swallowClick(props.id) === true) return;
          if (open !== null) control?.close("none");
          props.onStart();
        }}
        onKeyDown={(event) => {
          const step = rowMoveStep(event);
          if (step !== null && control !== null) {
            event.preventDefault();
            control.step(props.id, step);
            return;
          }
          if (event.key === "ArrowRight" && hasModels) {
            event.preventDefault();
            control.show(props.id, place());
          }
        }}
      >
        <Badge look={props.badge} />
        <span className="profile-name">{props.name}</span>
        {props.version !== null && (
          <span className="profile-version">
            <span aria-hidden="true">·</span> {props.version}
          </span>
        )}
        {props.updateAvailable && (
          <span className="dot update profile-update-dot" aria-hidden="true" />
        )}
        {/* The one hint that this row has more than its default: models open beside it. */}
        {hasModels && (
          <span className="profile-more">
            <ChevronIcon size={12} />
          </span>
        )}
      </button>
      {open !== null && (
        <ModelFlyout
          // A flyout the pointer opened turns into one with focus on Right arrow.
          key={open.focus ? "focus" : "hover"}
          program={props.name}
          sections={props.models}
          place={open.place}
          autoFocus={open.focus}
          anchor={row}
          onPick={(model) => {
            props.onStart(model);
          }}
          onBack={() => {
            // Opened from the row with the keyboard: back to the row. Opened by the pointer: the
            // focus goes back to where it was (the terminal), not to a row nobody focused.
            control?.close(open.focus ? (button.current ?? "none") : "restore");
          }}
          onClose={(reason) => {
            if (reason === "action") {
              control?.close("none");
              props.onMenuClose();
            } else {
              control?.close("restore");
            }
          }}
        />
      )}
      {props.children}
    </li>
  );
}
