import { useId, useMemo, useRef, useState } from "react";

import {
  entriesSince,
  entryHeading,
  initiallyOpenVersion,
  itemText,
  parseChangelog,
  toggleVersion,
  type ChangelogEntry,
} from "./changelog";
import changelogText from "./changelog.json?raw";
import { Dialog } from "./Dialog";
import { useT, type Language } from "./i18n";
import { ChevronIcon, CloseIcon } from "./icons";
import { VersionText } from "./VersionText";

/** Checked once on load; invalid entries are dropped (changelog.ts). */
export const CHANGELOG: readonly ChangelogEntry[] = parseChangelog(changelogText);

interface Props {
  language: Language;
  /** Only entries after this version up to `current`; `null` shows all. */
  since: string | null;
  /** Running version of Hangar, `null` until known. */
  current: string | null;
  onClose: () => void;
}

/** "What's New": versions newest first, items as a list, in the UI language. */
export function WhatsNewDialog({ language, since, current, onClose }: Props) {
  const t = useT();
  const body = useRef<HTMLElement>(null);
  const [showAll, setShowAll] = useState(since === null || current === null);
  const entries = useMemo(
    () =>
      showAll || since === null || current === null
        ? CHANGELOG
        : entriesSince(CHANGELOG, since, current),
    [showAll, since, current],
  );
  // `null` while nobody has toggled anything: the default applies and follows "All changes" too.
  const [openState, setOpenState] = useState<ReadonlySet<string> | null>(null);
  const defaultOpen = initiallyOpenVersion(CHANGELOG, entries);
  const collapsible = defaultOpen !== null;
  const open: ReadonlySet<string> = openState ?? new Set(defaultOpen === null ? [] : [defaultOpen]);
  const idBase = useId();

  return (
    <Dialog labelledBy="whats-new-title" className="whats-new" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="whats-new-title">{t("whatsNew.title")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <p className="whats-new-subtitle muted">{t("whatsNew.subtitle")}</p>

      <section
        ref={body}
        className="whats-new-body"
        aria-labelledby="whats-new-changes"
        tabIndex={0}
      >
        <h3 id="whats-new-changes" className="whats-new-heading">
          {t("whatsNew.changes")}
        </h3>
        {entries.length === 0 && <p className="muted">{t("whatsNew.empty")}</p>}
        {entries.map((entry) => {
          const heading = entryHeading(entry, current, language);
          // Without a date the version has not been released. If it is the running one (dev build),
          // "this version" says enough; an extra "in preparation" would be redundant.
          const date = heading.date ?? (heading.current ? null : t("whatsNew.upcoming"));
          // Same form as in the sidebar; the running version carries its build number.
          const label = (
            <>
              <VersionText version={heading.version} />
              {date !== null && <span className="whats-new-date">{date}</span>}
              {heading.current && (
                <span className="whats-new-current">{t("whatsNew.thisVersion")}</span>
              )}
            </>
          );
          const items = (
            <ul
              id={`${idBase}-${entry.version}`}
              className="whats-new-items"
              hidden={collapsible && !open.has(entry.version)}
            >
              {entry.items.map((item, index) => (
                // Items have no ID of their own; their order never changes at runtime.
                <li key={index} lang={item[language] === undefined ? "en" : language}>
                  {itemText(item, language)}
                </li>
              ))}
            </ul>
          );
          if (!collapsible) {
            return (
              <article key={entry.version} className="whats-new-release">
                <h4 className="whats-new-version">{label}</h4>
                {items}
              </article>
            );
          }
          const expanded = open.has(entry.version);
          return (
            <article key={entry.version} className="whats-new-release collapsible">
              <h4 className="whats-new-version">
                <button
                  type="button"
                  className="group-toggle whats-new-toggle"
                  aria-expanded={expanded}
                  aria-controls={`${idBase}-${entry.version}`}
                  onClick={() => {
                    setOpenState(toggleVersion(open, entry.version));
                  }}
                >
                  <ChevronIcon size={12} />
                  {label}
                  {!expanded && (
                    <span className="whats-new-count">
                      {t("whatsNew.itemCount", { count: entry.items.length })}
                    </span>
                  )}
                </button>
              </h4>
              {items}
            </article>
          );
        })}
      </section>

      <div className="dialog-actions whats-new-actions">
        {!showAll && (
          <button
            type="button"
            className="secondary-button"
            onClick={() => {
              setShowAll(true);
              // The button disappears; focus moves to the now complete list.
              requestAnimationFrame(() => body.current?.focus());
            }}
          >
            {t("whatsNew.showAll")}
          </button>
        )}
        <button type="button" className="primary-button" data-autofocus onClick={onClose}>
          {t("common.close")}
        </button>
      </div>
    </Dialog>
  );
}
