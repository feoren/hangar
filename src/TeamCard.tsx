import { useId, type ReactNode } from "react";

/**
 * One framed column of the Team window: a header bar with the title, an optional count and an
 * optional action, above a body that scrolls on its own. The header stays in place because it
 * sits outside the scrolling body.
 */
export function TeamCard(props: {
  title: ReactNode;
  /** How many entries the list holds; hidden at zero, where the empty line says it in words. */
  count?: number;
  /** Shown at the end of the header bar: one quiet button or label. */
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  const titleId = useId();
  return (
    <section
      className={props.className === undefined ? "team-card" : `team-card ${props.className}`}
      aria-labelledby={titleId}
    >
      <header className="team-card-head">
        <div className="team-card-label">
          <h4 id={titleId} className="team-card-title">
            {props.title}
          </h4>
          {/* Outside the heading, so the section is named by its title alone; the list below
              already tells a screen reader how many entries it holds. */}
          {props.count !== undefined && props.count > 0 && (
            <span className="team-card-count" aria-hidden="true">
              {props.count}
            </span>
          )}
        </div>
        {props.action}
      </header>
      <div className="team-card-body">{props.children}</div>
    </section>
  );
}

/** The friendly line an empty column shows, centered in its frame. */
export function TeamCardEmpty({ children: text }: { children: ReactNode }) {
  return <p className="team-help team-card-empty">{text}</p>;
}
