import { useT } from "./i18n";
import { AlertIcon, BellIcon, CheckIcon } from "./icons";
import { CHIP_LABELS, type ChipKind } from "./sessionChip";

/**
 * Status chip at the end of a session row's title line. Decorative for screen readers: the row's
 * status line already says the same in words. `compact` (collapsed rail) shows the shape only.
 */
export function StatusChip({ kind, compact = false }: { kind: ChipKind; compact?: boolean }) {
  const t = useT();
  const label = CHIP_LABELS[kind];
  return (
    <span className={`status-chip ${kind}${compact ? " compact" : ""}`} aria-hidden="true">
      <ChipShape kind={kind} />
      {!compact && label !== null && <span className="status-chip-label">{t(label)}</span>}
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
