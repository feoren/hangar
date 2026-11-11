import { AlertIcon, BellIcon, CheckIcon } from "./icons";
import type { RailChipKind } from "./sessionChip";

/**
 * Small status chip at the corner of a badge in the collapsed rail: the state's shape only.
 * Decorative for screen readers: the rail button's name already says the same in words.
 */
export function StatusChip({ kind }: { kind: RailChipKind }) {
  return (
    <span className={`status-chip ${kind}`} aria-hidden="true">
      <ChipShape kind={kind} />
    </span>
  );
}

function ChipShape({ kind }: { kind: RailChipKind }) {
  switch (kind) {
    case "done":
      return <CheckIcon size={12} />;
    case "asks":
      return <BellIcon size={12} />;
    case "failed":
      return <AlertIcon size={12} />;
  }
}
