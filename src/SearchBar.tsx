import { useEffect, useRef, useState } from "react";

import { useT, type Translate } from "./i18n";
import { ArrowDownIcon, ArrowUpIcon, CloseIcon } from "./icons";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import type { SearchResults, SessionTerminal } from "./terminal";

interface Props {
  terminal: SessionTerminal;
  /** Changes on every Ctrl+Shift+F, so a second press focuses and selects the field again. */
  focusToken: number;
  onClose: () => void;
}

/**
 * Search in the scrollback of the visible session (Ctrl+Shift+F). Searches while typing, starting
 * at the newest output; Enter goes up to older hits, Shift+Enter down to newer ones, Escape
 * closes and hands the keyboard back to the terminal.
 */
export function SearchBar({ terminal, focusToken, onClose }: Props) {
  const t = useT();
  const input = useRef<HTMLInputElement>(null);
  const [term, setTerm] = useState(() => terminal.searchSeed());
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [results, setResults] = useState<SearchResults | null>(null);

  useEffect(() => {
    terminal.setSearchListener(setResults);
    return () => {
      terminal.setSearchListener(null);
      terminal.clearSearch();
    };
  }, [terminal]);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [focusToken]);

  // A term taken over from the selection is searched right away.
  const initialTerm = useRef(term);
  useEffect(() => {
    if (initialTerm.current !== "") {
      terminal.find(initialTerm.current, {
        caseSensitive: false,
        backwards: true,
        incremental: true,
      });
    }
  }, [terminal]);

  // New output arrives at the bottom, so Enter searches upwards, towards older output.
  const go = (up: boolean) => {
    terminal.find(term, { caseSensitive, backwards: up, incremental: false });
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key === "Enter" && event.target === input.current) {
      event.preventDefault();
      go(!event.shiftKey);
    }
  };

  const status = statusText(term, results, t);
  const findKeys = formatShortcut(SHORTCUT_LABELS.find, t);

  return (
    <div className="search-bar" role="search" aria-label={t("search.label")} onKeyDown={onKeyDown}>
      <input
        ref={input}
        type="text"
        className="search-input"
        value={term}
        placeholder={t("search.placeholder")}
        aria-label={t("search.label")}
        aria-describedby="search-status"
        title={`${t("search.label")} (${findKeys})`}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => {
          const value = event.currentTarget.value;
          setTerm(value);
          terminal.find(value, { caseSensitive, backwards: true, incremental: true });
        }}
      />
      <span id="search-status" className="search-status" role="status" aria-live="polite">
        {status}
      </span>
      <button
        type="button"
        className={caseSensitive ? "icon-button toggle on search-case" : "icon-button search-case"}
        aria-pressed={caseSensitive}
        aria-label={t("search.caseSensitive")}
        title={t("search.caseSensitive")}
        onClick={() => {
          const next = !caseSensitive;
          setCaseSensitive(next);
          terminal.find(term, { caseSensitive: next, backwards: true, incremental: true });
          input.current?.focus();
        }}
      >
        <span aria-hidden="true">Aa</span>
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("search.up")}
        title={t("search.up")}
        disabled={term === ""}
        onClick={() => {
          go(true);
        }}
      >
        <ArrowUpIcon />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("search.down")}
        title={t("search.down")}
        disabled={term === ""}
        onClick={() => {
          go(false);
        }}
      >
        <ArrowDownIcon />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("search.close")}
        title={t("search.close")}
        onClick={onClose}
      >
        <CloseIcon />
      </button>
    </div>
  );
}

function statusText(term: string, results: SearchResults | null, t: Translate): string {
  if (term === "" || results === null) return "";
  if (results.count === 0) return t("search.none");
  if (results.index < 0) return t("search.many", { count: results.count });
  return t("search.position", { index: results.index + 1, count: results.count });
}
