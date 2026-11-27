// The programs in "New session". Each row is one start button: a click starts a session with the
// program's default. A row with models to choose from (ADR 0022) opens them in a flyout beside
// it once the pointer rests on it, or with Right arrow; a click there starts with that model. No
// extra glyph or pill: the row keeps its hover look while its flyout is open. Touch screens have
// no hover, so there a tap simply starts. Anything below a row (an update offer) comes in as
// children and wraps onto its own line.

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type { SessionModel } from "./api";
import { Badge } from "./Badge";
import { useT } from "./i18n";
import type { ModelSection } from "./localLlm";
import { headingFor, type PointerPoint } from "./menuNav";
import { flyoutPlace, ModelFlyout, type FlyoutPlace } from "./ModelMenu";
import type { BadgeLook } from "./programPrefs";

/** How long the pointer rests on a row before its flyout opens: passing over it does nothing. */
export const FLYOUT_OPEN_DELAY_MS = 150;
/**
 * How long an open flyout waits after the pointer left its row (on the way into the flyout), and
 * how long another row waits while the pointer crosses it heading for the flyout.
 */
export const FLYOUT_GRACE_MS = 300;

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
}

const FlyoutContext = createContext<FlyoutControl | null>(null);

/**
 * The list of program rows. It owns which flyout is open, so there is only ever one, and the
 * pointer can move from one row's flyout to the next row's without waiting again.
 */
export function ProgramList(props: { children: ReactNode }) {
  const [open, setOpen] = useState<OpenFlyout | null>(null);
  const openRef = useRef<OpenFlyout | null>(null);
  const rows = useRef(new Map<string, HTMLLIElement>());
  const pending = useRef<Pending | null>(null);
  const lastPoint = useRef<PointerPoint | null>(null);
  /** Where the focus was when the first flyout opened. */
  const returnTo = useRef<HTMLElement | null>(null);

  const control = useMemo((): Omit<FlyoutControl, "open"> => {
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

  const value = useMemo(() => ({ ...control, open }), [control, open]);
  return (
    <FlyoutContext.Provider value={value}>
      <ul ref={list} className="profile-list">
        {props.children}
      </ul>
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
      className="profile-row"
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
        title={`${label}\n${props.commandLine}`}
        aria-haspopup={hasModels ? "menu" : undefined}
        aria-expanded={hasModels ? open !== null : undefined}
        onClick={() => {
          if (open !== null) control?.close("none");
          props.onStart();
        }}
        onKeyDown={(event) => {
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
