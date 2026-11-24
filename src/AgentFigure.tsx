// A sub-agent's figure: one of the bundled figures (src/assets/sub-agents, built by
// tools/figures.py), an own image the backend checked, or the initial on the agent's color.
// No CSS moves a figure. A whole bundled figure may play its own calm idle loop, an animated WebP
// (figureMotion.ts decides when). The whole figure (`full`) is a portrait box, 2 : 3 like the
// bundled files (tools/figures.py); where there is no room for that, `head` shows the face in a
// round box.

import { useRef, type CSSProperties } from "react";

import { animatedFigureUrl, builtinFigureUrl } from "./figureFiles";
import { figureSource } from "./figureMotion";
import { useDecodedSource, useFiguresMayMove, useOnScreen } from "./useFigureMotion";
import { useSubAgentLibrary } from "./subAgentLibraryStore";
import {
  defaultFigure,
  FIGURE_PADDING,
  figureFor,
  figureTop,
  libraryCovers,
  runningFigure,
  type Figure,
} from "./subAgentLibrary";

/** Inline style plus the head crop's lift, read by `.agent-figure.head img` in styles.css. */
type FigureStyle = CSSProperties & { "--figure-lift"?: string };

interface Props {
  figure: Figure;
  /**
   * `full`: the height in CSS pixels, the width is two thirds of it; `head`: the edge of the
   * round face. Missing: the surrounding CSS sets it (a session's look).
   */
  size?: number;
  /** `full`: the whole figure; `head`: the face only, for places too small for the figure. */
  crop?: "head" | "full";
  /** Without a label the figure is decoration next to the agent's name. */
  label?: string;
}

export function AgentFigure({ figure, size, crop = "full", label }: Props) {
  const classes = `agent-figure ${crop === "head" ? "head" : "full"}`;
  const style: FigureStyle =
    size === undefined
      ? {}
      : { width: crop === "head" ? size : Math.round((size * 2) / 3), height: size };
  const a11y =
    label === undefined ? { "aria-hidden": true as const } : { role: "img", "aria-label": label };
  if (figure.kind === "initial" && crop === "full") {
    return (
      <span className={`${classes} guest`} style={style} {...a11y}>
        <GuestFigure letter={figure.letter} color={figure.color} />
      </span>
    );
  }
  if (figure.kind === "initial") {
    // The letter is as large as half the box's width.
    const fontSize =
      typeof style.width === "number" ? { fontSize: Math.round(style.width * 0.5) } : {};
    return (
      <span
        className={`${classes} initial`}
        style={{ ...style, ...fontSize, background: figure.color }}
        {...a11y}
      >
        {figure.letter}
      </span>
    );
  }
  const url = figure.kind === "builtin" ? builtinFigureUrl(figure.id) : figure.url;
  // A wide bundled figure sits lower in its file; the face crop moves it up by the difference.
  const lift =
    figure.kind === "builtin" && crop === "head" ? figureTop(figure.id) - FIGURE_PADDING : 0;
  const lifted: FigureStyle = lift > 0 ? { ...style, "--figure-lift": String(lift) } : style;
  // Only a whole bundled figure moves. The face crop of 14 to 26 px would show the idle loop as a
  // flicker of a few pixels in busy places (pane headers, search results) and decode 80 frames
  // for it; an own image is shown as the user gave it.
  const animated =
    figure.kind === "builtin" && crop === "full" ? animatedFigureUrl(figure.id) : undefined;
  return (
    <span
      className={figure.kind === "image" ? `${classes} custom` : classes}
      style={lifted}
      {...a11y}
    >
      {url !== undefined &&
        (animated === undefined ? (
          <img src={url} alt="" draggable={false} />
        ) : (
          <MovingImage still={url} animated={animated} />
        ))}
    </span>
  );
}

/** Outline of the guest figure, the dark ink line the bundled figures are drawn with. */
const INK = "#1b1712";

/**
 * The whole figure of an agent that is not one of Hangar's (`defaultFigure`): a small visitor in
 * the bundled figures' style, big head, ink outline, dressed in the agent's color, with a visitor
 * badge on a lanyard that carries the initial. Drawn on the bundled figures' 240 x 360 grid and
 * standing on its lower edge, so the stage and the sidebar place it like any other figure.
 */
function GuestFigure({ letter, color }: { letter: string; color: string }) {
  const light = `color-mix(in srgb, ${color} 10%, #f2d2b6)`;
  const dark = `color-mix(in srgb, ${color} 55%, ${INK})`;
  return (
    <svg viewBox="0 0 240 360" className="guest-figure">
      <g stroke={INK} strokeWidth={5} strokeLinejoin="round">
        <rect x={88} y={280} width={28} height={56} rx={10} style={{ fill: dark }} />
        <rect x={124} y={280} width={28} height={56} rx={10} style={{ fill: dark }} />
        <rect x={80} y={328} width={40} height={20} rx={10} fill="#2e251d" />
        <rect x={120} y={328} width={40} height={20} rx={10} fill="#2e251d" />
        <rect
          x={58}
          y={186}
          width={24}
          height={84}
          rx={12}
          transform="rotate(10 70 186)"
          style={{ fill: color }}
        />
        <rect
          x={158}
          y={186}
          width={24}
          height={84}
          rx={12}
          transform="rotate(-10 170 186)"
          style={{ fill: color }}
        />
        <circle cx={57} cy={268} r={13} style={{ fill: light }} />
        <circle cx={183} cy={268} r={13} style={{ fill: light }} />
        <path
          d="M80 192 Q80 178 96 176 L144 176 Q160 178 160 192 L166 282 Q166 292 156 292 L84 292 Q74 292 74 282 Z"
          style={{ fill: color }}
        />
        <rect x={107} y={156} width={26} height={24} rx={6} style={{ fill: light }} />
        <circle cx={120} cy={100} r={68} style={{ fill: light }} />
        <path
          d="M52 102 A68 68 0 0 1 188 102 C176 84 154 74 132 82 C112 70 78 78 52 102 Z"
          style={{ fill: dark }}
        />
      </g>
      <ellipse cx={92} cy={64} rx={20} ry={10} fill="#fff" opacity={0.16} />
      <ellipse cx={96} cy={114} rx={9} ry={12} fill={INK} />
      <ellipse cx={144} cy={114} rx={9} ry={12} fill={INK} />
      <circle cx={99} cy={109} r={3.5} fill="#fff" />
      <circle cx={147} cy={109} r={3.5} fill="#fff" />
      <ellipse cx={80} cy={134} rx={9} ry={5} fill="#ff8f7a" opacity={0.35} />
      <ellipse cx={160} cy={134} rx={9} ry={5} fill="#ff8f7a" opacity={0.35} />
      <path
        d="M111 136 Q120 144 129 136"
        fill="none"
        stroke={INK}
        strokeWidth={4}
        strokeLinecap="round"
      />
      <path
        d="M102 178 L114 212 M138 178 L126 212"
        fill="none"
        stroke={INK}
        strokeWidth={4}
        strokeLinecap="round"
      />
      <rect
        x={95}
        y={208}
        width={50}
        height={54}
        rx={9}
        fill="#f6f1e7"
        stroke={INK}
        strokeWidth={5}
      />
      <rect x={111} y={214} width={18} height={5} rx={2.5} fill={INK} />
      <text
        x={120}
        y={251}
        textAnchor="middle"
        fontSize={30}
        fontWeight={700}
        fontFamily="system-ui, sans-serif"
        style={{ fill: dark }}
      >
        {letter}
      </text>
    </svg>
  );
}

/**
 * A figure that plays its idle loop while figures may move and it is on screen, and shows its
 * still file otherwise. The next file is decoded before the swap (`useDecodedSource`), and both
 * hold the figure in the same place, so the swap neither blinks nor moves it.
 */
function MovingImage({ still, animated }: { still: string; animated: string }) {
  const ref = useRef<HTMLImageElement>(null);
  const mayMove = useFiguresMayMove();
  const onScreen = useOnScreen(ref, mayMove);
  const src = useDecodedSource(figureSource(still, animated, mayMove, onScreen));
  return <img ref={ref} src={src} alt="" draggable={false} />;
}

/**
 * The figure of a known sub-agent definition by name (`agentType` of a running sub-agent) in a
 * session started in `cwd` (`runningFigure`), `null` when no definition of that name is listed.
 */
export function useKnownFigure(name: string | null, cwd: string | null): Figure | null {
  const { library, figures, read } = useSubAgentLibrary();
  if (name === null) return null;
  return runningFigure(library, figures, name, cwd, read);
}

/**
 * The figure of a running sub-agent of type `name` for the crew: a known definition's figure like
 * `useKnownFigure`, else the figure the name alone points to (a built-in type such as `Explore`).
 * In a folder the listing does not cover, never the chosen one (`runningFigure`).
 */
export function useCrewFigure(name: string | null, cwd: string | null): Figure {
  const known = useKnownFigure(name, cwd);
  const { figures, read } = useSubAgentLibrary();
  if (known !== null) return known;
  const agent = name ?? "agent";
  return cwd !== null && !libraryCovers(read, cwd)
    ? defaultFigure(agent, null)
    : figureFor(agent, null, figures);
}
