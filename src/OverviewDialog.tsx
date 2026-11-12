import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { gridColumns, gridMove, isGridKey, TILE_REFRESH_MS } from "./overview";
import { matchShortcut } from "./shortcuts";

export interface OverviewTile {
  id: number;
  title: string;
  /** Badge of the session's program. */
  badge: React.ReactNode;
  /** CSS class of the state's edge color (`edge-*`, sessionChip.ts). */
  edge: string;
  /** The state in words, so color is never the only signal. */
  status: string;
}

interface Props {
  tiles: readonly OverviewTile[];
  activeId: number | null;
  /** The last lines of a session's terminal; called a few times per second while open. */
  readLines: (id: number) => string[];
  onActivate: (id: number) => void;
  onClose: () => void;
}

function sameLines(a: readonly string[] | undefined, b: readonly string[]): boolean {
  return a !== undefined && a.length === b.length && a.every((line, index) => line === b[index]);
}

/**
 * Overview (Ctrl+Shift+M): every session as a tile with its title, its state and the last lines of
 * its output, read from the terminal's buffer (no extra terminals). Arrows move, Enter opens the
 * session, Escape closes.
 */
export function OverviewDialog({ tiles, activeId, readLines, onActivate, onClose }: Props) {
  const t = useT();
  const id = useId();
  const grid = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [selected, setSelected] = useState(() =>
    Math.max(
      0,
      tiles.findIndex((tile) => tile.id === activeId),
    ),
  );
  const [lines, setLines] = useState<ReadonlyMap<number, readonly string[]>>(() => new Map());
  const reader = useRef(readLines);

  // Live, but calm: a few reads per second, and React renders only when a tile changed.
  const ids = tiles.map((tile) => tile.id).join(",");
  useEffect(() => {
    const read = () => {
      setLines((current) => {
        let changed = false;
        const next = new Map<number, readonly string[]>();
        for (const tileId of ids === "" ? [] : ids.split(",").map(Number)) {
          const fresh = reader.current(tileId);
          const old = current.get(tileId);
          if (old !== undefined && sameLines(old, fresh)) {
            next.set(tileId, old);
          } else {
            next.set(tileId, fresh);
            changed = true;
          }
        }
        return changed || next.size !== current.size ? next : current;
      });
    };
    read();
    const timer = window.setInterval(read, TILE_REFRESH_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [ids]);

  useLayoutEffect(() => {
    const element = grid.current;
    if (!element) return;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver(() => {
      setWidth(element.clientWidth);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  const columns = gridColumns(tiles.length, width);
  const current = Math.min(selected, tiles.length - 1);

  const focusTile = (index: number) => {
    setSelected(index);
    grid.current?.querySelectorAll<HTMLElement>(".overview-tile")[index]?.focus();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (matchShortcut(event) === "overview") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.ctrlKey || event.altKey || event.metaKey || !isGridKey(event.key)) return;
    event.preventDefault();
    const next = gridMove(current, tiles.length, columns, event.key);
    if (next >= 0) focusTile(next);
  };

  return (
    <Dialog labelledBy={`${id}-title`} className="overview" onClose={onClose}>
      <h2 id={`${id}-title`} className="overview-title">
        {t("overview.title")}
      </h2>
      {tiles.length === 0 ? (
        <p className="muted">{t("sidebar.noSessions")}</p>
      ) : (
        <div
          ref={grid}
          className="overview-grid"
          style={{ gridTemplateColumns: `repeat(${String(columns)}, minmax(0, 1fr))` }}
          onKeyDown={onKeyDown}
        >
          {tiles.map((tile, index) => (
            <button
              key={tile.id}
              type="button"
              className={`overview-tile ${tile.edge}`}
              tabIndex={index === current ? 0 : -1}
              aria-current={tile.id === activeId ? "true" : undefined}
              data-autofocus={index === current ? true : undefined}
              onFocus={() => {
                setSelected(index);
              }}
              onClick={() => {
                onActivate(tile.id);
              }}
            >
              <span className="overview-head">
                {tile.badge}
                <span className="overview-name">{tile.title}</span>
                <span className="overview-status">{tile.status}</span>
              </span>
              <pre className="overview-lines" aria-hidden="true">
                {(lines.get(tile.id) ?? []).join("\n")}
              </pre>
            </button>
          ))}
        </div>
      )}
      <p className="launcher-hint" aria-hidden="true">
        {t("overview.hint")}
      </p>
    </Dialog>
  );
}
