import { AlertIcon, BellIcon, CheckIcon } from "./icons";
import type { ChipKind } from "./sessionChip";

/**
 * Status icon at the top right of a session row: only the shape, in its state's color, without a
 * pill or a word. Decorative for screen readers: the row's status line already says the same in
 * words. `compact` (collapsed rail) sits as a small circle on the badge's corner.
 */
export function StatusChip({ kind, compact = false }: { kind: ChipKind; compact?: boolean }) {
  return (
    <span className={`status-chip ${kind}${compact ? " compact" : ""}`} aria-hidden="true">
      <ChipShape kind={kind} />
    </span>
  );
}

function ChipShape({ kind }: { kind: ChipKind }) {
  switch (kind) {
    case "working":
      return (
        <span className="status-chip-dots">
          <span />
          <span />
          <span />
        </span>
      );
    case "paused":
      return (
        <span className="status-chip-bars">
          <span />
          <span />
        </span>
      );
    case "done":
      return <CheckIcon size={11} />;
    case "asks":
      return <BellIcon size={11} />;
    case "failed":
      return <AlertIcon size={11} />;
    case "ended":
      return <span className="status-chip-square" />;
  }
}
