import { useEffect, useRef, useState } from "react";

import { useT } from "./i18n";
import { ratiosFor, type PaneRatios } from "./paneRatios";
import {
  LAYOUT_IDS,
  LAYOUT_NAMES,
  SNAP_TARGETS,
  sameTarget,
  stepTarget,
  targetIndex,
  zoneLabel,
  zones,
  type LayoutId,
  type SnapTarget,
  type Zone,
} from "./snapLayouts";

/** Position of a zone inside a thumbnail, with a hairline gap between zones. */
function zoneStyle(zone: Zone): React.CSSProperties {
  const gap = 1.5;
  const start = (offset: number) => (offset === 0 ? 0 : gap);
  const end = (offset: number, size: number) => (offset + size >= 0.999 ? 0 : gap);
  return {
    left: `calc(${String(zone.x * 100)}% + ${String(start(zone.x))}px)`,
    top: `calc(${String(zone.y * 100)}% + ${String(start(zone.y))}px)`,
    width: `calc(${String(zone.width * 100)}% - ${String(start(zone.x) + end(zone.x, zone.width))}px)`,
    height: `calc(${String(zone.height * 100)}% - ${String(start(zone.y) + end(zone.y, zone.height))}px)`,
  };
}

interface DragProps {
  /** A session is being dragged from the sidebar: the bar only shows, the pointer chooses. */
  mode: "drag";
  current: LayoutId;
  /** Splitter positions per layout: the thumbnails show the sizes each layout will have. */
  ratios: PaneRatios;
  /** Zone under the pointer. */
  highlight: SnapTarget | null;
}

interface PickProps {
  /** Without a drag: a small picker that the keyboard and the mouse operate. */
  mode: "pick";
  current: LayoutId;
  ratios: PaneRatios;
  /** Zone chosen first: where the active session is now. */
  initial: SnapTarget;
  /** Name of the session to place, for the caption. */
  title: string;
  /** Zone under the keyboard focus or the mouse, for the preview in the panes area. */
  onActive: (target: SnapTarget) => void;
  onPick: (target: SnapTarget) => void;
  /** Escape, a click outside or the focus leaving the picker. */
  onClose: () => void;
}

/**
 * The snap layout bar at the top of the panes area, like the snap layouts of Windows 11: one
 * thumbnail per layout, one zone per session. While dragging a session it marks the zone under
 * the pointer; as a picker, arrows move between the zones of all layouts, Enter places the active
 * session, Escape closes.
 */
export function SnapBar(props: DragProps | PickProps) {
  const t = useT();
  const pick = props.mode === "pick" ? props : null;
  const root = useRef<HTMLDivElement>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  const [active, setActive] = useState(() => {
    if (!pick) return 0;
    return Math.max(0, targetIndex(pick.initial));
  });
  const onActive = pick?.onActive;
  const onClose = pick?.onClose;

  useEffect(() => {
    if (!pick) return;
    buttons.current.get(active)?.focus();
    const target = SNAP_TARGETS[active];
    if (target) onActive?.(target);
    // Focus and preview follow the active zone only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  useEffect(() => {
    if (!onClose) return;
    const onPointer = (event: PointerEvent) => {
      if (event.target instanceof Node && root.current?.contains(event.target)) return;
      onClose();
    };
    document.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  const highlight = props.mode === "drag" ? props.highlight : (SNAP_TARGETS[active] ?? null);

  return (
    <div
      ref={root}
      className={`snap-bar ${props.mode}`}
      role={pick ? "dialog" : undefined}
      aria-label={pick ? t("snap.label") : undefined}
      aria-hidden={pick ? undefined : true}
      onKeyDown={
        pick
          ? (event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                pick.onClose();
                return;
              }
              const next = stepTarget(active, event.key);
              if (next === null) return;
              event.preventDefault();
              setActive(next);
            }
          : undefined
      }
      onBlur={
        pick
          ? (event) => {
              const next = event.relatedTarget;
              if (next instanceof Node && root.current?.contains(next)) return;
              pick.onClose();
            }
          : undefined
      }
    >
      {pick && <p className="snap-caption">{t("snap.place", { title: pick.title })}</p>}
      <div className="snap-layouts" role={pick ? "group" : undefined}>
        {LAYOUT_IDS.map((layout) => (
          <div
            key={layout}
            className={layout === props.current ? "snap-thumb current" : "snap-thumb"}
            title={pick ? undefined : t(LAYOUT_NAMES[layout])}
          >
            <div className="snap-zones" data-snap-layout={layout}>
              {zones(layout, ratiosFor(props.ratios, layout)).map((zone, index) => {
                const target: SnapTarget = { layout, zone: index };
                const lit = sameTarget(highlight, target);
                const className = lit ? "snap-zone lit" : "snap-zone";
                if (!pick) {
                  return <span key={index} className={className} style={zoneStyle(zone)} />;
                }
                const flat = targetIndex(target);
                return (
                  <button
                    key={index}
                    ref={(element) => {
                      if (element) buttons.current.set(flat, element);
                      else buttons.current.delete(flat);
                    }}
                    type="button"
                    className={className}
                    style={zoneStyle(zone)}
                    tabIndex={flat === active ? 0 : -1}
                    aria-label={zoneLabel(t, layout, index)}
                    title={zoneLabel(t, layout, index)}
                    onPointerEnter={() => {
                      setActive(flat);
                    }}
                    onClick={() => {
                      pick.onPick(target);
                    }}
                  />
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
