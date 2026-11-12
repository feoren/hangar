import { useDeferredValue, useEffect, useId, useMemo, useRef, useState } from "react";

import { Dialog } from "./Dialog";
import { highlightRuns } from "./fuzzy";
import { useT, type MessageKey } from "./i18n";
import {
  firstSelectable,
  flatEntries,
  moveSelection,
  rankEntries,
  type LauncherEntry,
  type LauncherSection,
} from "./launcher";

export interface LauncherItem extends LauncherEntry {
  /** Leading image, e.g. the program's badge. */
  icon?: React.ReactNode;
  run: () => void;
}

interface Props {
  /** Builds the entries once on open; quick access is short lived, so a snapshot is enough. */
  buildItems: () => LauncherItem[];
  /**
   * Entries that need the backend (files, commits), started once on open. Each adds its entries
   * when it arrives; a failed one adds nothing, the rest of quick access works as before.
   */
  loaders?: readonly (() => Promise<LauncherItem[]>)[];
  /** Closed without running anything (Escape, click outside); the caller returns focus. */
  onClose: () => void;
  /** An entry was chosen: the dialog is closed first, then `item.run()` follows. */
  onRun: (item: LauncherItem) => void;
}

const SECTION_NAMES: Record<LauncherSection, MessageKey> = {
  sessions: "sidebar.sessions",
  start: "launcher.start",
  files: "files.title",
  notes: "notes.title",
  commits: "changes.viewCommits",
  snippets: "settings.snippets",
  settings: "settings.title",
  commands: "launcher.commands",
};

/**
 * Quick access (Ctrl+Shift+P): one field for sessions, programs, files, notes, commits, snippets,
 * settings and commands; a few results per section. Arrows choose, Enter runs, Escape closes.
 * Combobox pattern: the focus stays in the field, the chosen entry is announced via
 * aria-activedescendant.
 */
export function LauncherDialog({ buildItems, loaders, onClose, onRun }: Props) {
  const t = useT();
  const id = useId();
  const [items, setItems] = useState(buildItems);
  const [query, setQuery] = useState("");
  // Ranking thousands of files may take a moment; typing stays immediate.
  const ranked = useDeferredValue(query);
  const sections = useMemo(() => rankEntries(items, ranked), [items, ranked]);
  const flat = useMemo(() => flatEntries(sections), [sections]);
  const [chosen, setSelected] = useState(() => firstSelectable(flat));
  const list = useRef<HTMLDivElement>(null);

  // Once on open; the loaders are a snapshot like the items.
  const startLoaders = useRef(loaders);
  useEffect(() => {
    let open = true;
    for (const load of startLoaders.current ?? []) {
      load()
        .then((more) => {
          if (open && more.length > 0) setItems((current) => [...current, ...more]);
        })
        .catch(() => {
          // Optional entries: without them quick access still finds everything else.
        });
    }
    return () => {
      open = false;
    };
  }, []);

  // New query: the best entry is chosen again (adjusted during render, no flash of the old one).
  // New items with the same query (a session changed state) keep the choice where it still fits.
  const [rankedFor, setRankedFor] = useState(ranked);
  if (rankedFor !== ranked) {
    setRankedFor(ranked);
    setSelected(firstSelectable(flat));
  }
  const selected =
    chosen >= 0 && chosen < flat.length && flat[chosen]?.disabled !== true
      ? chosen
      : firstSelectable(flat);

  const optionId = (index: number) => `${id}-option-${String(index)}`;

  useEffect(() => {
    if (selected < 0) return;
    list.current
      ?.querySelector(`#${CSS.escape(optionId(selected))}`)
      ?.scrollIntoView({ block: "nearest" });
    // optionId only depends on the stable id.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  const run = (index: number) => {
    const item = flat[index];
    if (!item || item.disabled === true) return;
    onRun(item);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    // Enter and the arrows belong to the input method while it composes (Japanese, Chinese).
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = moveSelection(flat, selected, event.key === "ArrowDown" ? 1 : -1);
      if (next >= 0) setSelected(next);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      run(selected);
    }
  };

  const positions = new Map(flat.map((entry, index) => [entry.key, index]));
  return (
    <Dialog labelledBy={`${id}-title`} className="launcher" onClose={onClose}>
      <h2 id={`${id}-title`} className="sr-only">
        {t("launcher.title")}
      </h2>
      <input
        className="launcher-input"
        type="text"
        role="combobox"
        aria-expanded={flat.length > 0}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={selected >= 0 ? optionId(selected) : undefined}
        aria-label={t("launcher.title")}
        placeholder={t("launcher.placeholder")}
        value={query}
        spellCheck={false}
        autoComplete="off"
        data-autofocus
        onChange={(event) => {
          setQuery(event.currentTarget.value);
        }}
        onKeyDown={onKeyDown}
      />
      <div ref={list} id={`${id}-list`} className="launcher-list" role="listbox">
        {sections.map((section) => {
          const headingId = `${id}-section-${section.section}`;
          return (
            <div key={section.section} role="group" aria-labelledby={headingId}>
              <div id={headingId} className="launcher-section" role="presentation">
                {t(SECTION_NAMES[section.section])}
              </div>
              {section.items.map((ranked) => {
                const item = ranked.entry;
                const position = positions.get(item.key) ?? -1;
                const isSelected = position === selected;
                return (
                  <div
                    key={item.key}
                    id={optionId(position)}
                    role="option"
                    aria-selected={isSelected}
                    aria-disabled={item.disabled === true ? true : undefined}
                    className={[
                      "launcher-option",
                      isSelected ? "selected" : "",
                      item.disabled === true ? "disabled" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onPointerMove={() => {
                      if (!isSelected && item.disabled !== true) setSelected(position);
                    }}
                    onMouseDown={(event) => {
                      // Keep the focus in the field.
                      event.preventDefault();
                    }}
                    onClick={() => {
                      run(position);
                    }}
                  >
                    {item.icon}
                    <span className="launcher-label">
                      {highlightRuns(item.label, ranked.positions).map((segment, part) =>
                        segment.match ? (
                          <mark key={part}>{segment.text}</mark>
                        ) : (
                          <span key={part}>{segment.text}</span>
                        ),
                      )}
                    </span>
                    {item.detail !== undefined && (
                      <span className="launcher-detail">{item.detail}</span>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
        {flat.length === 0 && <p className="launcher-none">{t("launcher.none")}</p>}
      </div>
      <p className="launcher-hint" aria-hidden="true">
        {t("launcher.hint")}
      </p>
    </Dialog>
  );
}
