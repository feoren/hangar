import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  buildCalendar,
  firstDayOfWeek,
  monthLabels,
  parseGitActivity,
  recentWeeks,
  weeksThatFit,
  type ActivityDay,
} from "./activity";
import { api } from "./api";
import { useT, type Language, type Translate } from "./i18n";

/** Cell size and gap in pixels; the same values as `.activity` in styles.css. */
const CELL = 14;
const GAP = 3;

function dayTitle(day: ActivityDay, date: Intl.DateTimeFormat, t: Translate): string {
  const text = date.format(day.date);
  if (day.count === 0) return t("activity.dayNone", { date: text });
  if (day.count === 1) return t("activity.dayOne", { date: text });
  return t("activity.dayMany", { count: day.count, date: text });
}

/**
 * The user's own commits of the last year in the folders Hangar knows, as a calendar of days.
 * Always shown on the home view: while loading, on failure or without any commit it shows the
 * empty year. Loads again when the set of folders changes (they can arrive after the first render).
 */
export function ActivityCalendar({
  folders,
  language,
}: {
  folders: readonly string[];
  language: Language;
}) {
  const t = useT();
  const [times, setTimes] = useState<readonly number[]>([]);
  const [width, setWidth] = useState(0);
  const box = useRef<HTMLElement | null>(null);
  // One string per folder set, so a new array with the same folders does not reload.
  const folderKey = folders.join("\n");

  useEffect(() => {
    const list = folderKey === "" ? [] : folderKey.split("\n");
    if (list.length === 0) return;
    let alive = true;
    api
      .gitActivity(list)
      .then((value) => {
        if (alive) setTimes(parseGitActivity(value) ?? []);
      })
      .catch(() => {
        // No git or no readable repository: the calendar stays as it is.
      });
    return () => {
      alive = false;
    };
  }, [folderKey]);

  const calendar = useMemo(
    () => buildCalendar(times, new Date(), firstDayOfWeek(navigator.language)),
    [times],
  );

  useLayoutEffect(() => {
    const element = box.current;
    if (element === null) return;
    setWidth(element.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      setWidth(element.clientWidth);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  const weeks = recentWeeks(calendar, weeksThatFit(width, CELL, GAP));
  const dateFormat = new Intl.DateTimeFormat(language, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
  const columns = { gridTemplateColumns: `repeat(${String(weeks.length)}, ${String(CELL)}px)` };
  const monthFormat = new Intl.DateTimeFormat(language, { month: "short" });
  const total =
    calendar.total === 0
      ? t("activity.totalNone")
      : calendar.total === 1
        ? t("activity.totalOne")
        : t("activity.totalMany", { count: calendar.total.toLocaleString(language) });

  return (
    <section className="activity" ref={box}>
      <div className="activity-calendar">
        <div className="activity-months" style={columns} aria-hidden="true">
          {monthLabels(weeks).map((label) => (
            <span key={label.week} style={{ gridColumnStart: label.week + 1 }}>
              {monthFormat.format(label.date)}
            </span>
          ))}
        </div>
        <div className="activity-days" style={columns} aria-hidden="true">
          {weeks.flatMap((column, week) =>
            column.map((day, index) =>
              day === null ? (
                <span key={`${String(week)}-${String(index)}`} className="activity-day blank" />
              ) : (
                <span
                  key={`${String(week)}-${String(index)}`}
                  className={`activity-day level-${String(day.level)}`}
                  title={dayTitle(day, dateFormat, t)}
                />
              ),
            ),
          )}
        </div>
      </div>
      <p className="activity-total">{total}</p>
    </section>
  );
}
