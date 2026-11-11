import { useT } from "./i18n";
import { GLYPH_SIZE, glyphRects, placementText, type PanePlacement } from "./paneGlyph";

/**
 * The current snap layout in miniature, the session's own zone filled, the others outlined: where
 * this session sits in the panes area. Shape and text, never color alone (title and label).
 */
export function LayoutGlyph({ placement }: { placement: PanePlacement }) {
  const t = useT();
  const text = placementText(t, placement);
  const { width, height } = GLYPH_SIZE;
  return (
    <span className="layout-glyph" role="img" aria-label={text} title={text}>
      <svg width={width} height={height} viewBox={`0 0 ${String(width)} ${String(height)}`}>
        {glyphRects(placement.layout, placement.ratios).map((rect, zone) =>
          zone === placement.zone ? (
            <rect
              key={zone}
              className="layout-glyph-own"
              x={rect.x}
              y={rect.y}
              width={rect.width}
              height={rect.height}
              rx={1}
            />
          ) : (
            // Inset by half the stroke, so the outline stays inside the zone's pixels.
            <rect
              key={zone}
              className="layout-glyph-other"
              x={rect.x + 0.5}
              y={rect.y + 0.5}
              width={Math.max(0, rect.width - 1)}
              height={Math.max(0, rect.height - 1)}
              rx={1}
            />
          ),
        )}
      </svg>
    </span>
  );
}
