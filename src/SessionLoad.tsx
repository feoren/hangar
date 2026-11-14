// The load half of a session row's third line: CPU, RAM and, where the machine gives GPU
// numbers, GPU of the session's process tree, each value in a cell of fixed width (styles.css,
// "Third line"). Pure: the row passes the formatted numbers (resources.ts).

import { useT } from "./i18n";
import { CpuIcon, GpuIcon, MemoryIcon } from "./icons";
import type { ResourceParts } from "./resources";

const RESOURCE_ICONS = { cpu: CpuIcon, memory: MemoryIcon, gpu: GpuIcon } as const;

/**
 * One column of the third line, "CPU 12 %": the name, then each value in a cell of fixed width,
 * right-aligned, so the columns line up from row to row (styles.css, "Third line"). In a very
 * narrow row the name gives way to a small icon (CSS); screen readers always hear the name. A
 * column without any value yet (CPU before the second reading, GPU while the tree leaves it alone)
 * keeps its width but shows nothing and says nothing.
 */
function ResourceFact({
  kind,
  name,
  values,
  spoken,
}: {
  kind: keyof typeof RESOURCE_ICONS;
  name: string;
  /** One per cell; each cell's width comes from its class (styles.css). */
  values: readonly { cell: "percent" | "memory"; text: string | null }[];
  /** What screen readers hear after the name; `null` for an empty column. */
  spoken: string | null;
}) {
  const Icon = RESOURCE_ICONS[kind];
  return (
    <span
      className={`session-resource session-resource-${kind}${spoken === null ? " vacant" : ""}`}
      aria-hidden={spoken === null ? true : undefined}
    >
      <span className="session-resource-name" aria-hidden="true">
        {name}
      </span>
      <span className="session-resource-icon" aria-hidden="true">
        <Icon size={10} />
      </span>
      {values.map(({ cell, text }) => (
        <span key={cell} className={`session-value session-value-${cell}`} aria-hidden="true">
          {text}
        </span>
      ))}
      {spoken !== null && <span className="sr-only">{`${name} ${spoken}`}</span>}
    </span>
  );
}

/**
 * The load half of the third line: CPU, RAM and, where the machine gives GPU numbers, GPU. The
 * GPU column stays (empty) for a session without GPU use, so the run time after it does not move.
 */
export function SessionLoad({ usage, gpuColumn }: { usage: ResourceParts; gpuColumn: boolean }) {
  const t = useT();
  return (
    <span className="session-load">
      <ResourceFact
        kind="cpu"
        name={t("system.cpu")}
        values={[{ cell: "percent", text: usage.cpu }]}
        spoken={usage.cpu}
      />
      <ResourceFact
        kind="memory"
        name={t("system.memory")}
        values={[{ cell: "memory", text: usage.memory }]}
        spoken={usage.memory}
      />
      {(gpuColumn || usage.gpu !== null) && (
        <ResourceFact
          kind="gpu"
          name={t("system.gpu")}
          values={[
            { cell: "percent", text: usage.gpuLoad },
            { cell: "memory", text: usage.gpuMemory },
          ]}
          spoken={usage.gpu}
        />
      )}
    </span>
  );
}
