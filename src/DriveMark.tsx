// Hangar Drive's logo in the sidebar (ADR 0024): with a small "+" it backs the project up, with a
// check it says Hangar Drive holds the project (a pause or stop sign while it is paused or
// stopped). Nothing shows where Hangar Drive cannot take the folder (hangarDrive.ts `driveMark`);
// the session menu offers the same hand-over.

import type { KeyboardEvent, SyntheticEvent } from "react";

import logoSvg from "./assets/hangar-drive/hangar-drive.svg?raw";
import { driveMarkKey, type DriveMark, type DriveState } from "./hangarDrive";
import { useT } from "./i18n";

interface Shape {
  kind: "rect" | "path";
  attributes: Readonly<Record<string, string>>;
}

/** The logo's shapes with their own attributes, read from the bundled file (NOTICE.md there). */
function shapesOf(svg: string): Shape[] {
  return [...svg.matchAll(/<(rect|path)\s([^>]*?)\/>/g)].map((match) => ({
    kind: match[1] === "rect" ? "rect" : "path",
    attributes: Object.fromEntries(
      [...(match[2] ?? "").matchAll(/([a-z-]+)="([^"]*)"/g)].map((pair) => [
        pair[1] ?? "",
        pair[2] ?? "",
      ]),
    ),
  }));
}

const SHAPES = shapesOf(logoSvg);

/** Hangar Drive's logo, in its own colors: the folded D on its dark tile. Decorative. */
export function HangarDriveLogo({ size = 14 }: { size?: number }) {
  return (
    <svg
      className="hangar-drive-logo"
      width={size}
      height={size}
      viewBox="0 0 256 256"
      aria-hidden="true"
    >
      {SHAPES.map((shape, index) => {
        const a = shape.attributes;
        return shape.kind === "rect" ? (
          <rect key={index} width={a.width} height={a.height} rx={a.rx} fill={a.fill} />
        ) : (
          <path
            key={index}
            d={a.d}
            fill={a.fill}
            fillRule={a["fill-rule"] === "evenodd" ? "evenodd" : undefined}
          />
        );
      })}
    </svg>
  );
}

type BadgeKind = "add" | DriveState;

/** The small sign on the logo's corner: plus, check, pause or stop, drawn in an 8 px circle. */
function Badge({ kind }: { kind: BadgeKind }) {
  const look = kind === "add" ? "add" : kind === "paused" || kind === "stopped" ? "quiet" : "ok";
  return (
    <span className={`drive-badge ${look}`} aria-hidden="true">
      <svg
        viewBox="0 0 8 8"
        width="8"
        height="8"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {kind === "add" && <path d="M4 2v4M2 4h4" />}
        {kind === "paused" && <path d="M3 2.4v3.2M5 2.4v3.2" />}
        {kind === "stopped" && <rect x="2.6" y="2.6" width="2.8" height="2.8" rx="0.4" />}
        {(kind === "active" || kind === "other") && <path d="M2.2 4.2l1.2 1.1 2.4-2.6" />}
      </svg>
    </span>
  );
}

/** The logo with its badge, sized for where it sits. */
function Sign({ kind, size }: { kind: BadgeKind; size: number }) {
  return (
    <span className="drive-sign">
      <HangarDriveLogo size={size} />
      <Badge kind={kind} />
    </span>
  );
}

/** Keeps an event from reaching the session row around the logo (activate, drag, rename). */
function stop(event: SyntheticEvent) {
  event.stopPropagation();
}

/**
 * The logo for `mark`. `inRow`: it sits inside the session row's own button, where HTML allows
 * no second button, so the way to back up is a focusable element with the button role that keeps
 * every click and key from the row; elsewhere it is a real icon button.
 */
export function DriveMarkView({
  mark,
  onBackUp,
  inRow = false,
  size = 14,
  className = "",
}: {
  mark: DriveMark;
  /** Hands the folder to Hangar Drive, once per click; Hangar Drive asks the person to confirm. */
  onBackUp: () => void;
  inRow?: boolean;
  size?: number;
  /** Where it sits, for the layout (styles.css, "Hangar Drive logo"). */
  className?: string;
}) {
  const t = useT();
  const classes = (kind: string) => ["drive-mark", kind, className].filter(Boolean).join(" ");
  if (mark.kind === "held") {
    const text = t(driveMarkKey(mark.state));
    return (
      <span className={classes(`held ${mark.state}`)} title={text}>
        <Sign kind={mark.state} size={size} />
        <span className="sr-only">{text}</span>
      </span>
    );
  }
  const label = t("drive.backUp");
  if (!inRow) {
    return (
      <button
        type="button"
        className={classes("offer icon-button")}
        aria-label={label}
        title={label}
        onClick={onBackUp}
      >
        <Sign kind="add" size={size} />
      </button>
    );
  }
  return (
    <span
      role="button"
      tabIndex={0}
      className={classes("offer")}
      aria-label={label}
      title={label}
      onClick={(event) => {
        event.stopPropagation();
        event.preventDefault();
        onBackUp();
      }}
      onPointerDown={stop}
      onMouseDown={stop}
      onDoubleClick={stop}
      onKeyDown={(event: KeyboardEvent) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.stopPropagation();
        event.preventDefault();
        if (!event.repeat) onBackUp();
      }}
      onKeyUp={(event: KeyboardEvent) => {
        // The row's button would take a released space as its own click.
        if (event.key === " ") {
          event.stopPropagation();
          event.preventDefault();
        }
      }}
    >
      <Sign kind="add" size={size} />
    </span>
  );
}
