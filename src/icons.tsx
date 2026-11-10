// Small stroke icons, so no icon library is needed.

interface IconProps {
  size?: number;
}

function Svg({ size = 16, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export function CloseIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 4l8 8M12 4l-8 8" />
    </Svg>
  );
}

export function StopIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="4" y="4" width="8" height="8" rx="1.5" />
    </Svg>
  );
}

export function RestartIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 8a5 5 0 1 0 1.6-3.7" />
      <path d="M3 2.5v3h3" />
    </Svg>
  );
}

export function PlusIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 3.5v9M3.5 8h9" />
    </Svg>
  );
}

export function FolderIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.5 1.6h4.9A1.5 1.5 0 0 1 14 6.1v5.4a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z" />
    </Svg>
  );
}

export function MenuIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.5 4h11M2.5 8h11M2.5 12h11" />
    </Svg>
  );
}

export function CollapseIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M6 2.5v11M10.5 6.5 9 8l1.5 1.5" />
    </Svg>
  );
}

export function ExpandIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M6 2.5v11M9 6.5 10.5 8 9 9.5" />
    </Svg>
  );
}

export function DownloadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 2.5v7.5M5 7l3 3 3-3M3 13h10" />
    </Svg>
  );
}

export function EditIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10.5 3.2l2.3 2.3M3 13l.6-2.9 7.6-7.6a1.2 1.2 0 0 1 1.7 0l.6.6a1.2 1.2 0 0 1 0 1.7l-7.6 7.6z" />
    </Svg>
  );
}

/**
 * "Folded H" logo (output/design/hangar-logo/03-gefaltetes-h.svg) without the dark tile,
 * so it sits directly on the sidebar. Purely decorative: the name is next to it or in the tooltip.
 */
export function HangarLogo({ size = 18 }: IconProps) {
  return (
    <svg
      className="logo"
      width={size}
      height={size}
      viewBox="32 32 192 192"
      aria-hidden="true"
      focusable="false"
    >
      <path className="logo-h" d="M40 72L80 58V120L176 84V72L216 58V200H176V120L80 156V200H40Z" />
      <path className="logo-bar" d="M110 172H150V190H110Z" />
    </svg>
  );
}

export function SunIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="8" r="2.8" />
      <path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M3.4 12.6l1.1-1.1M11.5 4.5l1.1-1.1" />
    </Svg>
  );
}

export function MoonIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M13.2 9.6A5.6 5.6 0 0 1 6.4 2.8a5.6 5.6 0 1 0 6.8 6.8z" />
    </Svg>
  );
}

/** Arrow of the collapsible groups; points right and is rotated via CSS. */
export function ChevronIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M6 4l4 4-4 4" />
    </Svg>
  );
}

export function ArrowUpIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 13V3M4 7l4-4 4 4" />
    </Svg>
  );
}

export function ArrowDownIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 3v10M4 9l4 4 4-4" />
    </Svg>
  );
}

/** Drag handle (two rows of dots). */
export function GripIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M6 4h.01M10 4h.01M6 8h.01M10 8h.01M6 12h.01M10 12h.01" strokeWidth="2.4" />
    </Svg>
  );
}

export function CopyIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 3.5v-.5A1.5 1.5 0 0 0 9 1.5H3A1.5 1.5 0 0 0 1.5 3v6A1.5 1.5 0 0 0 3 10.5h.5" />
    </Svg>
  );
}

/** Notepad with lines: show/hide the notes panel. */
export function NotesIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 2.5h6l2.5 2.5v8.5H4z" />
      <path d="M6.2 7h4M6.2 9.4h4M6.2 11.8h2.4" />
    </Svg>
  );
}

/** Eye: preview. */
export function EyeIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M1.8 8s2.3-4.3 6.2-4.3S14.2 8 14.2 8s-2.3 4.3-6.2 4.3S1.8 8 1.8 8z" />
      <circle cx="8" cy="8" r="1.9" />
    </Svg>
  );
}

export function TrashIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5M7 7v4M9 7v4" />
    </Svg>
  );
}

export function CheckIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.5 8.5l3 3 6-7" />
    </Svg>
  );
}

/** Favorite folder; `filled` marks the current folder as a favorite. */
export function StarIcon({ filled = false, ...props }: IconProps & { filled?: boolean }) {
  return (
    <Svg {...props}>
      <path
        d="M8 2.2l1.75 3.6 3.95.55-2.87 2.77.7 3.93L8 11.2l-3.53 1.85.7-3.93L2.3 6.35l3.95-.55z"
        fill={filled ? "currentColor" : "none"}
      />
    </Svg>
  );
}

/** Recently used. */
export function ClockIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M8 4.8V8l2.2 1.4" />
    </Svg>
  );
}

/** Git branch (status line). */
export function BranchIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="5" cy="3.8" r="1.6" />
      <circle cx="5" cy="12.2" r="1.6" />
      <circle cx="11" cy="5.8" r="1.6" />
      <path d="M5 5.4v5.2M11 7.4c0 2.4-2.2 3-6 3.4" />
    </Svg>
  );
}

/** A session's own worktree: a branch growing out of a folder. */
export function WorktreeIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2 5.5V12a1 1 0 0 0 1 1h4.5M2 5.5V4a1 1 0 0 1 1-1h2.6l1.4 1.5H9" />
      <circle cx="12" cy="4.5" r="1.6" />
      <circle cx="12" cy="11.5" r="1.6" />
      <path d="M12 6.1v3.8" />
    </Svg>
  );
}

/** The program rang the bell: it probably asks something (status chip). */
export function BellIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4.2 11V7.4a3.8 3.8 0 0 1 7.6 0V11l1.2 1.4H3z" />
      <path d="M6.6 13.6a1.5 1.5 0 0 0 2.8 0" />
    </Svg>
  );
}

/** A process failed (status chip): an exclamation mark in a circle. */
export function AlertIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 4.8v3.6" />
      <path d="M8 11.1v.1" />
    </Svg>
  );
}

/** Magnifier: search. */
export function SearchIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="7" cy="7" r="4.2" />
      <path d="M10.2 10.2l3.3 3.3" />
    </Svg>
  );
}

/** Three dots: "More" menu. */
export function MoreIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.5 8h.01M8 8h.01M12.5 8h.01" strokeWidth="2.6" />
    </Svg>
  );
}

/** Arrow into a prompt: send to the terminal. */
export function SendIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.5 8h9M8 4.5L11.5 8 8 11.5M13.5 3.5v9" />
    </Svg>
  );
}

/** Box with an arrow out of it: open in its own window. */
export function DetachIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M9.5 2.5h4v4M13.5 2.5L8 8" />
      <path d="M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" />
    </Svg>
  );
}

/** Pin: keep the window on top. */
export function PinIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M6 2.5h4M7 2.5v4L4.5 9.5h7L9 6.5v-4M8 9.5v4" />
    </Svg>
  );
}

/** Ticked box: checklist progress. */
export function TaskIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="2.5" />
      <path d="M5.3 8.2l1.9 1.9 3.6-4" />
    </Svg>
  );
}

/** Two sessions touch the same file: two overlapping sheets. */
export function OverlapIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2" y="2.5" width="7.5" height="9" rx="1.2" />
      <path d="M6.5 11.5v1a1 1 0 0 0 1 1H13a1 1 0 0 0 1-1v-7a1 1 0 0 0-1-1h-3.5" />
    </Svg>
  );
}

/** Gear: settings. */
export function GearIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12.70 6.31 14.46 6.64 14.46 9.36 12.70 9.69 12.52 10.13 13.53 11.61 11.61 13.53 10.13 12.52 9.69 12.70 9.36 14.46 6.64 14.46 6.31 12.70 5.87 12.52 4.39 13.53 2.47 11.61 3.48 10.13 3.30 9.69 1.54 9.36 1.54 6.64 3.30 6.31 3.48 5.87 2.47 4.39 4.39 2.47 5.87 3.48 6.31 3.30 6.64 1.54 9.36 1.54 9.69 3.30 10.13 3.48 11.61 2.47 13.53 4.39 12.52 5.87Z" />
      <circle cx="8" cy="8" r="2.1" />
    </Svg>
  );
}

/** One person: an account without a name or picture, or signing in. */
export function PersonIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="5.6" r="2.6" />
      <path d="M2.8 13.6c.6-2.5 2.7-4 5.2-4s4.6 1.5 5.2 4" />
    </Svg>
  );
}

/** Two people: the team. */
export function TeamIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="6" cy="5.8" r="2.3" />
      <path d="M1.8 13.4c.5-2.2 2.1-3.5 4.2-3.5s3.7 1.3 4.2 3.5" />
      <path d="M10.4 3.7a2.3 2.3 0 0 1 0 4.3" />
      <path d="M11.6 9.9c1.4.4 2.3 1.6 2.6 3.5" />
    </Svg>
  );
}
