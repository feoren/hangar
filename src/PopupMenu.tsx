import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import { CloseIcon } from "./icons";
import {
  firstIndex,
  fitInViewport,
  menuCommand,
  reorderDrop,
  secondaryCommand,
  swatchCommand,
  type NavEntry,
} from "./menuNav";
import { beyondThreshold } from "./sessionDrag";
import { rowMoveStep } from "./shortcuts";

/** A button at the end of a row, e.g. the favorite star, a folder's color or "+ Worktree". */
export interface PopupSecondary {
  label: string;
  icon: React.ReactNode;
  /** Toggle state (favorite star): announced as a checkbox item. */
  checked?: boolean;
  /** Marks a button whose look carries its own color (a folder's color dot). */
  className?: string;
  /** The menu stays open (toggles); otherwise it closes like for the row. */
  keepOpen?: boolean;
  run: () => void;
}

/**
 * A row the person can move among the other rows that have one (favorite folders): by dragging
 * it, or with Alt+Up and Alt+Down while it has the focus. The caller moves it; the focus follows
 * the row's key.
 */
export interface PopupReorder {
  onStep: (step: 1 | -1) => void;
  onDrop: (targetKey: string, after: boolean) => void;
}

/** One choice in a row of swatches; `color` is a CSS color, `null` for "none". */
export interface PopupSwatch {
  key: string;
  label: string;
  color: string | null;
}

export type PopupItem =
  | {
      type: "action";
      key: string;
      label: string;
      /** Dimmed suffix after the name, e.g. the version. */
      detail?: string;
      shortcut?: string;
      /** Leading image, e.g. a program's badge. */
      icon?: React.ReactNode;
      /** Tooltip, e.g. the full path behind a shortened label. */
      title?: string;
      disabled?: boolean;
      /** Marks the row (e.g. the current folder); read out as "checked". */
      current?: boolean;
      /**
       * Buttons at the row's end, reached with Right arrow; the first one also with Shift+Enter
       * and Shift+click on the row.
       */
      secondary?: PopupSecondary | readonly PopupSecondary[];
      /**
       * Takes the row out of its list (a folder from the menu): a small x at the row's end, shown
       * on hover and focus, and the Delete key on the row. The menu stays open.
       */
      remove?: { label: string; run: () => void };
      reorder?: PopupReorder;
      run: () => void;
    }
  | { type: "separator"; key: string }
  /** Small caption above a group of entries; not focusable. */
  | { type: "heading"; key: string; label: string }
  /**
   * A short sentence in the menu, e.g. why an option is missing; not focusable. `describes`
   * names the key of the entry it explains: screen readers read it along with that entry.
   */
  | { type: "note"; key: string; label: string; describes?: string }
  /**
   * A row of color swatches below the entry `owner` (a folder's color). It takes the focus when
   * it appears; Left and Right choose, Enter picks, Escape closes it and the focus returns to its
   * entry.
   */
  | {
      type: "swatches";
      key: string;
      label: string;
      owner: string;
      options: readonly PopupSwatch[];
      selected: string;
      onPick: (key: string) => void;
      onDismiss: () => void;
    };

interface Props {
  items: readonly PopupItem[];
  /** Name of the menu for screen readers. */
  label: string;
  /** Top left corner in the window (clientX/clientY); the menu moves into the window if space is short. */
  position: { x: number; y: number };
  /**
   * Opened from a button near the bottom: if the menu does not fit below `position`, its bottom
   * edge goes to this y instead (above the button) rather than covering the button.
   */
  flipAbove?: number;
  className?: string;
  /** Button that opens the menu: clicking it toggles instead of closing and reopening. */
  anchor?: React.RefObject<HTMLElement | null>;
  /**
   * Closed after an action (`action`) or via Escape, Tab, a click outside, a window switch
   * (`dismiss`). The caller returns focus.
   */
  onClose: (reason: "action" | "dismiss") => void;
}

function secondariesOf(item: PopupItem | undefined): readonly PopupSecondary[] {
  if (item?.type !== "action" || item.secondary === undefined) return [];
  const secondary = item.secondary;
  return "run" in secondary ? [secondary] : secondary;
}

/** A drag of a reorderable row, from pointer down to release. */
interface RowDrag {
  key: string;
  pointerId: number;
  startX: number;
  startY: number;
  started: boolean;
  drop: { key: string; after: boolean } | null;
  stop: (commit: boolean) => void;
}

/**
 * Floating single-level menu (terminal context menu, program picker of the collapsed sidebar,
 * folder menu). Same keyboard control as the app menu: focus on the first entry, arrows, Home,
 * End, first letter, Enter, Escape; Right and Left reach a row's buttons.
 */
export function PopupMenu({
  items,
  label,
  position,
  flipAbove,
  className,
  anchor,
  onClose,
}: Props) {
  const entries: NavEntry[] = items.map((item) => {
    if (item.type === "swatches") return { label: item.label, enabled: true, submenu: false };
    return item.type !== "action"
      ? null
      : { label: item.label, enabled: item.disabled !== true, submenu: false };
  });
  const [current, setCurrent] = useState(() => firstIndex(entries));
  /** Focused button at the end of the current row; -1: the row itself. */
  const [secondaryAt, setSecondaryAt] = useState(-1);
  /** Focused swatch while a swatch row has the focus. */
  const [swatchAt, setSwatchAt] = useState(0);
  const [placed, setPlaced] = useState(position);
  /** Row being dragged and where it would land, for the drop line. */
  const [dragView, setDragView] = useState<{
    key: string;
    drop: { key: string; after: boolean } | null;
  } | null>(null);
  const panel = useRef<HTMLUListElement>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  /** Buttons at a row's end, by `${index}:${position}`. */
  const secondaries = useRef(new Map<string, HTMLButtonElement>());
  const swatchButtons = useRef(new Map<number, HTMLButtonElement>());
  const closeRef = useRef(onClose);
  /** Key of the focused entry, so the focus follows it when rows move or appear. */
  const focusKey = useRef<string | null>(items[current]?.key ?? null);
  /** The open swatch row and its entry, to notice it appearing and closing. */
  const openSwatches = useRef<{ key: string; owner: string } | null>(null);
  const rowDrag = useRef<RowDrag | null>(null);
  /** After a drag, the click that ends it must not choose the row. */
  const swallowClick = useRef(false);
  const noteId = useId();
  /** Note ids per described entry key. */
  const notesFor = new Map<string, string[]>();
  items.forEach((item, index) => {
    if (item.type !== "note" || item.describes === undefined) return;
    const ids = notesFor.get(item.describes) ?? [];
    ids.push(`${noteId}-${String(index)}`);
    notesFor.set(item.describes, ids);
  });
  useEffect(() => {
    closeRef.current = onClose;
  });

  // Measure before the first paint and push into the window, so nothing jumps.
  useLayoutEffect(() => {
    const element = panel.current;
    if (element) {
      const rect = element.getBoundingClientRect();
      const fitsBelow = position.y + rect.height + 8 <= window.innerHeight;
      const start =
        !fitsBelow && flipAbove !== undefined
          ? { x: position.x, y: flipAbove - rect.height }
          : position;
      setPlaced(
        fitInViewport(
          start,
          { width: rect.width, height: rect.height },
          { width: window.innerWidth, height: window.innerHeight },
        ),
      );
    }
    const first = buttons.current.get(current);
    if (first) first.focus();
    else element?.focus();
    // Only on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const dismiss = () => {
      closeRef.current("dismiss");
    };
    const onPointer = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && panel.current?.contains(target)) return;
      if (target instanceof Node && anchor?.current?.contains(target)) return;
      dismiss();
    };
    document.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
      rowDrag.current?.stop(false);
    };
  }, [anchor]);

  /** Puts the focus on an entry: the row, one of its buttons (`at`) or a swatch (`swatch`). */
  const focusAt = (index: number, at: number, swatch = 0) => {
    setCurrent(index);
    setSecondaryAt(at);
    setSwatchAt(swatch);
    focusKey.current = items[index]?.key ?? null;
    const item = items[index];
    const element =
      item?.type === "swatches"
        ? swatchButtons.current.get(swatch)
        : at >= 0
          ? secondaries.current.get(`${String(index)}:${String(at)}`)
          : buttons.current.get(index);
    element?.focus();
  };

  // Rows that move, appear or go: the focus stays with its entry, a new swatch row takes it,
  // and a closed swatch row hands it back to its entry.
  useLayoutEffect(() => {
    const swatchIndex = items.findIndex((item) => item.type === "swatches");
    const swatches = items[swatchIndex];
    const before = openSwatches.current;
    openSwatches.current =
      swatches?.type === "swatches" ? { key: swatches.key, owner: swatches.owner } : null;
    if (swatches?.type === "swatches" && before?.key !== swatches.key) {
      const selected = swatches.options.findIndex((option) => option.key === swatches.selected);
      focusAt(swatchIndex, -1, Math.max(0, selected));
      return;
    }
    let index = items.findIndex((item) => item.key === focusKey.current);
    let at = secondaryAt;
    if (index < 0 && before !== null && focusKey.current === before.key) {
      // Picked or closed: back on the entry the swatches belong to.
      index = items.findIndex((item) => item.key === before.owner);
      at = -1;
    }
    if (index < 0) return;
    const active = document.activeElement;
    const lost = active === null || active === document.body || active === panel.current;
    if (index !== current || lost) focusAt(index, at, swatchAt);
    // Only when the items change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

  const activate = (index: number) => {
    const item = items[index];
    if (!item || item.type !== "action" || item.disabled === true) return;
    onClose("action");
    item.run();
  };

  const runSecondary = (index: number, at: number) => {
    const item = items[index];
    const button = secondariesOf(item)[at];
    if (!item || item.type !== "action" || item.disabled === true || !button) return;
    if (button.keepOpen !== true) onClose("action");
    button.run();
  };

  const runRemove = (index: number) => {
    const item = items[index];
    if (!item || item.type !== "action" || !item.remove) return;
    item.remove.run();
    // The row is gone; the keyboard stays in the menu.
    panel.current?.focus();
  };

  /** Pointer down on a reorderable row: becomes a drag once it moves past the threshold. */
  const beginDrag = (key: string, event: React.PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0 || !event.isPrimary || rowDrag.current !== null) return;
    const element = event.currentTarget;
    const pointerId = event.pointerId;
    try {
      element.setPointerCapture(pointerId);
    } catch {
      return;
    }
    const rows = () =>
      items.flatMap((item, index) => {
        if (item.type !== "action" || item.reorder === undefined) return [];
        const rect = buttons.current.get(index)?.getBoundingClientRect();
        return rect === undefined ? [] : [{ key: item.key, mid: rect.top + rect.height / 2 }];
      });
    const onMove = (move: PointerEvent) => {
      const drag = rowDrag.current;
      if (drag === null || move.pointerId !== pointerId) return;
      if (!drag.started) {
        if (!beyondThreshold(move.clientX - drag.startX, move.clientY - drag.startY)) return;
        drag.started = true;
      }
      drag.drop = reorderDrop(rows(), key, move.clientY);
      setDragView({ key, drop: drag.drop });
    };
    const onUp = (up: PointerEvent) => {
      if (up.pointerId === pointerId) stop(true);
    };
    const onCancel = () => {
      stop(false);
    };
    const stop = (commit: boolean) => {
      const drag = rowDrag.current;
      if (drag === null) return;
      rowDrag.current = null;
      element.removeEventListener("pointermove", onMove);
      element.removeEventListener("pointerup", onUp);
      element.removeEventListener("pointercancel", onCancel);
      element.removeEventListener("lostpointercapture", onCancel);
      try {
        if (element.hasPointerCapture(pointerId)) element.releasePointerCapture(pointerId);
      } catch {
        // The row may be gone already.
      }
      if (!drag.started) return;
      setDragView(null);
      // The click that follows the release belongs to the drag.
      swallowClick.current = true;
      window.setTimeout(() => {
        swallowClick.current = false;
      }, 0);
      const item = items.find((entry) => entry.key === key);
      if (commit && drag.drop !== null && item?.type === "action") {
        focusKey.current = key;
        item.reorder?.onDrop(drag.drop.key, drag.drop.after);
      }
    };
    rowDrag.current = {
      key,
      pointerId,
      startX: event.clientX,
      startY: event.clientY,
      started: false,
      drop: null,
      stop,
    };
    element.addEventListener("pointermove", onMove);
    element.addEventListener("pointerup", onUp);
    element.addEventListener("pointercancel", onCancel);
    element.addEventListener("lostpointercapture", onCancel);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    event.stopPropagation();
    if (event.key === "Escape" && rowDrag.current?.started === true) {
      event.preventDefault();
      rowDrag.current.stop(false);
      return;
    }
    const item = items[current];
    if (item?.type === "swatches") {
      const command = swatchCommand(event, swatchAt, item.options.length);
      if (command !== null) {
        event.preventDefault();
        if (command.kind === "focus") focusAt(current, -1, command.index);
        else if (command.kind === "pick") item.onPick(item.options[swatchAt]?.key ?? "");
        else item.onDismiss();
        return;
      }
    }
    if (item?.type === "action" && item.reorder !== undefined && secondaryAt < 0) {
      const step = rowMoveStep(event);
      if (step !== null) {
        event.preventDefault();
        item.reorder.onStep(step);
        return;
      }
    }
    if (event.key === "Delete" && item?.type === "action" && item.remove) {
      event.preventDefault();
      runRemove(current);
      return;
    }
    const second = secondaryCommand(event, secondariesOf(item).length, secondaryAt);
    if (second !== null) {
      event.preventDefault();
      if (second.kind === "focus") focusAt(current, second.at);
      else runSecondary(current, second.at);
      return;
    }
    const command = menuCommand(event, entries, current, false);
    switch (command.kind) {
      case "none":
        return;
      case "focus":
        focusAt(command.index, -1);
        break;
      case "activate":
        activate(current);
        break;
      case "openSubmenu":
      case "closeSubmenu":
        return;
      case "closeAll":
        onClose("dismiss");
        break;
    }
    event.preventDefault();
  };

  /** Classes of a row while one is dragged: the dragged one fades, one shows the drop line. */
  const dragClass = (key: string) => {
    if (dragView === null) return "";
    if (dragView.key === key) return " dragging";
    if (dragView.drop?.key === key) return dragView.drop.after ? " drop-after" : " drop-before";
    return "";
  };

  return (
    <ul
      ref={panel}
      className={className ? `menu-panel popup-menu ${className}` : "menu-panel popup-menu"}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={{ left: placed.x, top: placed.y }}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      {items.map((item, index) => {
        if (item.type === "separator") {
          return <li key={item.key} role="separator" className="menu-separator" />;
        }
        if (item.type === "heading") {
          return (
            <li key={item.key} role="presentation" className="menu-heading">
              {item.label}
            </li>
          );
        }
        if (item.type === "note") {
          return (
            <li
              key={item.key}
              id={`${noteId}-${String(index)}`}
              role="presentation"
              className="menu-note"
            >
              {item.label}
            </li>
          );
        }
        if (item.type === "swatches") {
          return (
            <li key={item.key} role="none" className="menu-swatches">
              <div role="group" aria-label={item.label} className="menu-swatch-row">
                {item.options.map((option, at) => (
                  <button
                    key={option.key}
                    type="button"
                    role="menuitemradio"
                    aria-checked={option.key === item.selected}
                    aria-label={option.label}
                    title={option.label}
                    ref={(element) => {
                      if (element) swatchButtons.current.set(at, element);
                      else swatchButtons.current.delete(at);
                    }}
                    tabIndex={index === current && at === swatchAt ? 0 : -1}
                    className={option.color === null ? "menu-swatch none" : "menu-swatch"}
                    onPointerEnter={() => {
                      focusAt(index, -1, at);
                    }}
                    onClick={() => {
                      item.onPick(option.key);
                    }}
                  >
                    <span
                      className="menu-swatch-color"
                      style={option.color === null ? undefined : { background: option.color }}
                      aria-hidden="true"
                    />
                  </button>
                ))}
              </div>
            </li>
          );
        }
        const buttonsAtEnd = secondariesOf(item);
        const remove = item.remove;
        return (
          <li
            key={item.key}
            className={
              (buttonsAtEnd.length > 0 || remove
                ? `menu-entry with-secondary${remove ? " removable" : ""}`
                : "menu-entry") +
              (item.reorder ? " reorderable" : "") +
              dragClass(item.key)
            }
            role="none"
          >
            <button
              type="button"
              role={item.current === undefined ? "menuitem" : "menuitemradio"}
              aria-checked={item.current}
              aria-describedby={notesFor.get(item.key)?.join(" ")}
              ref={(element) => {
                if (element) buttons.current.set(index, element);
                else buttons.current.delete(index);
              }}
              tabIndex={index === current && secondaryAt < 0 ? 0 : -1}
              className={item.current === true ? "menu-item current" : "menu-item"}
              title={item.title}
              disabled={item.disabled}
              onPointerEnter={() => {
                if (item.disabled === true || rowDrag.current?.started === true) return;
                focusAt(index, -1);
              }}
              onPointerDown={
                item.reorder === undefined
                  ? undefined
                  : (event) => {
                      beginDrag(item.key, event);
                    }
              }
              onClick={(event) => {
                if (swallowClick.current) {
                  swallowClick.current = false;
                  return;
                }
                if (event.shiftKey && buttonsAtEnd.length > 0) runSecondary(index, 0);
                else activate(index);
              }}
            >
              {item.icon}
              <span className="menu-label">
                {item.label}
                {item.detail && (
                  <span className="menu-detail">
                    <span aria-hidden="true"> · </span>
                    {item.detail}
                  </span>
                )}
              </span>
              <span className="menu-hint">{item.shortcut ?? ""}</span>
            </button>
            {buttonsAtEnd.map((button, at) => (
              <button
                key={button.label}
                type="button"
                role={button.checked === undefined ? "menuitem" : "menuitemcheckbox"}
                aria-checked={button.checked}
                aria-label={button.label}
                title={button.label}
                ref={(element) => {
                  const id = `${String(index)}:${String(at)}`;
                  if (element) secondaries.current.set(id, element);
                  else secondaries.current.delete(id);
                }}
                tabIndex={index === current && secondaryAt === at ? 0 : -1}
                className={[
                  "menu-secondary icon-button",
                  button.checked === true ? "on" : "",
                  button.className ?? "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                disabled={item.disabled}
                onPointerEnter={() => {
                  if (item.disabled === true) return;
                  focusAt(index, at);
                }}
                onClick={() => {
                  runSecondary(index, at);
                }}
              >
                {button.icon}
              </button>
            ))}
            {remove && (
              <button
                type="button"
                className="menu-remove icon-button"
                aria-label={remove.label}
                title={remove.label}
                tabIndex={-1}
                onClick={() => {
                  runRemove(index);
                }}
              >
                <CloseIcon size={12} />
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
