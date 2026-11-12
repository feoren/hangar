import { useEffect, useId, useMemo, useRef, useState } from "react";

import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import {
  excerpt,
  searchTargets,
  SEARCH_LIMIT,
  type LineMatch,
  type SearchTarget,
} from "./sessionSearch";

interface Props {
  /** Session names, in the order the results appear (the visible session first). */
  sessions: readonly { id: number; title: string }[];
  /** The buffers to search, read when a search starts (they change while output arrives). */
  targets: () => SearchTarget[];
  /** Start term, e.g. the selection of the visible terminal. */
  seed: string;
  /** Leading image per session, e.g. its program's badge. */
  badge: (sessionId: number) => React.ReactNode;
  onClose: () => void;
  /** A hit was chosen: the dialog is closed first, then the caller shows that spot. */
  onReveal: (match: LineMatch) => void;
}

/** Wait after the last key before searching, so typing a word starts one search, not five. */
const TYPING_PAUSE_MS = 120;

type Results =
  | { kind: "idle" }
  | { kind: "searching"; query: string; matches: readonly LineMatch[] }
  | { kind: "done"; query: string; matches: readonly LineMatch[]; truncated: boolean };

const IDLE: Results = { kind: "idle" };
const NO_MATCHES: readonly LineMatch[] = [];

/** Gives the browser a turn between chunks, so typing and the terminals stay responsive. */
function pause(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}

/**
 * Search across sessions (Ctrl+Shift+S): one field, hits from the output of every open session
 * grouped by session, each with the line around the hit. Arrows choose, Enter goes there, Escape
 * closes. Combobox pattern like quick access.
 */
export function SessionSearchDialog({ sessions, targets, seed, badge, onClose, onReveal }: Props) {
  const t = useT();
  const id = useId();
  const [query, setQuery] = useState(seed);
  const [results, setResults] = useState<Results>({ kind: "idle" });
  const [selected, setSelected] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  // A snapshot like quick access; the function itself reads the buffers anew on every search.
  const readTargets = useRef(targets);

  useEffect(() => {
    // A blank field shows nothing (see `shown`); there is nothing to search.
    if (query.trim() === "") return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      setResults((current) => ({
        kind: "searching",
        query,
        matches: current.kind === "idle" ? [] : current.matches,
      }));
      void searchTargets(
        readTargets.current(),
        query,
        {},
        {
          pause,
          cancelled: () => cancelled,
          onProgress: (matches) => {
            if (!cancelled) setResults({ kind: "searching", query, matches: [...matches] });
          },
        },
      )
        .then((result) => {
          if (cancelled || result.cancelled) return;
          setResults({
            kind: "done",
            query,
            matches: result.matches,
            truncated: result.truncated,
          });
        })
        .catch(() => {
          // A terminal closed while it was being read: end the search instead of spinning forever.
          if (!cancelled) setResults({ kind: "done", query, matches: [], truncated: false });
        });
    }, TYPING_PAUSE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const shown: Results = query.trim() === "" ? IDLE : results;
  const matches = shown.kind === "idle" ? NO_MATCHES : shown.matches;
  // A new query starts at the first hit again.
  const [selectedFor, setSelectedFor] = useState(query);
  if (selectedFor !== query) {
    setSelectedFor(query);
    setSelected(0);
  }
  const current = matches.length === 0 ? -1 : Math.min(selected, matches.length - 1);

  const groups = useMemo(() => {
    const names = new Map(sessions.map((session) => [session.id, session.title]));
    const out: {
      sessionId: number;
      title: string;
      items: { match: LineMatch; index: number }[];
    }[] = [];
    matches.forEach((match, index) => {
      let group = out[out.length - 1];
      if (group?.sessionId !== match.sessionId) {
        group = {
          sessionId: match.sessionId,
          title: names.get(match.sessionId) ?? `#${String(match.sessionId)}`,
          items: [],
        };
        out.push(group);
      }
      group.items.push({ match, index });
    });
    return out;
  }, [matches, sessions]);

  const optionId = (index: number) => `${id}-hit-${String(index)}`;

  useEffect(() => {
    if (current < 0) return;
    list.current
      ?.querySelector(`#${CSS.escape(optionId(current))}`)
      ?.scrollIntoView({ block: "nearest" });
    // optionId only depends on the stable id.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current]);

  const reveal = (index: number) => {
    const match = matches[index];
    if (match) onReveal(match);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    const count = matches.length;
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && count > 0) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((((current + step) % count) + count) % count);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      reveal(current);
    }
  };

  let status = "";
  if (shown.kind === "searching" && shown.matches.length === 0) {
    status = t("searchAll.searching");
  } else if (shown.kind === "done" && shown.matches.length === 0) {
    status = t("searchAll.none");
  } else if (shown.kind === "done" && shown.truncated) {
    status = t("searchAll.more", { count: SEARCH_LIMIT });
  } else if (shown.kind !== "idle") {
    status = t("searchAll.count", { count: shown.matches.length });
  }

  return (
    <Dialog labelledBy={`${id}-title`} className="launcher session-search" onClose={onClose}>
      <h2 id={`${id}-title`} className="sr-only">
        {t("searchAll.title")}
      </h2>
      <input
        className="launcher-input"
        type="text"
        role="combobox"
        aria-expanded={matches.length > 0}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={current >= 0 ? optionId(current) : undefined}
        aria-label={t("searchAll.title")}
        placeholder={t("searchAll.placeholder")}
        value={query}
        spellCheck={false}
        autoComplete="off"
        data-autofocus
        onChange={(event) => {
          setQuery(event.currentTarget.value);
        }}
        onFocus={(event) => {
          event.currentTarget.select();
        }}
        onKeyDown={onKeyDown}
      />
      <div ref={list} id={`${id}-list`} className="launcher-list" role="listbox">
        {groups.map((group) => {
          const headingId = `${id}-session-${String(group.sessionId)}`;
          return (
            <div key={group.sessionId} role="group" aria-labelledby={headingId}>
              <div id={headingId} className="launcher-section search-session" role="presentation">
                {badge(group.sessionId)}
                <span>{group.title}</span>
              </div>
              {group.items.map(({ match, index }) => {
                const piece = excerpt(match.text, match.start, match.length);
                const isSelected = index === current;
                return (
                  <div
                    key={`${String(match.row)}:${String(index)}`}
                    id={optionId(index)}
                    role="option"
                    aria-selected={isSelected}
                    className={
                      isSelected
                        ? "launcher-option search-hit selected"
                        : "launcher-option search-hit"
                    }
                    onPointerMove={() => {
                      if (!isSelected) setSelected(index);
                    }}
                    onMouseDown={(event) => {
                      // Keep the focus in the field.
                      event.preventDefault();
                    }}
                    onClick={() => {
                      reveal(index);
                    }}
                  >
                    <span className="search-excerpt">
                      {piece.before}
                      <mark>{piece.match}</mark>
                      {piece.after}
                    </span>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
      <p className="launcher-hint search-status" role="status">
        {status === "" ? t("searchAll.hint") : `${status} · ${t("searchAll.hint")}`}
      </p>
    </Dialog>
  );
}
