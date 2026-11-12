import { useT } from "./i18n";
import {
  barPending,
  formatShare,
  GAUGES,
  gaugeWarns,
  SPARK_HEIGHT,
  SPARK_WIDTH,
  sparkPaths,
  type Gauge,
  type History,
} from "./systemUsage";
import { useSystemUsage } from "./useSystemUsage";

const NAME_KEYS = { cpu: "system.cpu", memory: "system.memory", gpu: "system.gpu" } as const;

/**
 * One row above New session: CPU, RAM and GPU of the whole machine, each a sparkline of the last
 * minute and the current share. A value the platform cannot give hides its gauge; with none at
 * all (setting off, first reading pending) the row is gone.
 */
export function SystemBar({ language }: { language: string }) {
  const t = useT();
  const { usage, history, tick } = useSystemUsage();
  const shown = GAUGES.filter((gauge) => usage[gauge] !== null);
  // The very first reading knows memory only (CPU and GPU are rates and need a second one):
  // waiting for it keeps the bar from showing RAM alone and then jumping to three gauges.
  if (shown.length === 0 || barPending(usage, tick)) return null;
  return (
    <div className="system-bar" role="group" aria-label={t("system.usage")}>
      {shown.map((gauge) => (
        <SystemGauge
          key={gauge}
          gauge={gauge}
          value={usage[gauge] ?? 0}
          history={history[gauge]}
          tick={tick}
          name={t(NAME_KEYS[gauge])}
          language={language}
        />
      ))}
    </div>
  );
}

function SystemGauge({
  gauge,
  value,
  history,
  tick,
  name,
  language,
}: {
  gauge: Gauge;
  value: number;
  history: History;
  tick: number;
  name: string;
  language: string;
}) {
  const t = useT();
  const text = formatShare(value, language);
  const paths = sparkPaths(history);
  return (
    <span className={gaugeWarns(value) ? `gauge gauge-${gauge} warn` : `gauge gauge-${gauge}`}>
      <span className="gauge-name" aria-hidden="true">
        {name}
      </span>
      <span className="gauge-graph" aria-hidden="true">
        {/* Animated: slides one reading to the left over the poll interval. */}
        <svg
          className="gauge-spark"
          viewBox={`0 0 ${String(SPARK_WIDTH)} ${String(SPARK_HEIGHT)}`}
          preserveAspectRatio="none"
        >
          {paths !== null && (
            // A new key per reading restarts the scroll animation.
            <g key={tick} className="gauge-scroll">
              <path className="gauge-area" d={paths.area} />
              <path className="gauge-line" d={paths.line} />
            </g>
          )}
        </svg>
        {/* Reduced motion: the current share as a still bar instead. */}
        <span className="gauge-bar">
          <span className="gauge-fill" style={{ width: `${String(value)}%` }} />
        </span>
      </span>
      <span className="gauge-value" aria-hidden="true">
        {text}
      </span>
      <span className="sr-only">{t("system.gauge", { name, value: text })}</span>
    </span>
  );
}
