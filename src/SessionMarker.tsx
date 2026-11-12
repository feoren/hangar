import { useT, type Translate } from "./i18n";
import { hasMarker, markerColorName, type Marker } from "./marker";

/** Spoken name of the marker color, e.g. "Marker red"; empty without a color. */
export function markerColorText(marker: Marker | undefined, t: Translate): string {
  const color = marker?.color ?? null;
  if (color === null) return "";
  const name = markerColorName(color);
  return t("marker.colorLabel", { color: name === null ? t("marker.custom") : t(name) });
}

/**
 * Session marker before the title: a small color dot and the emoji. The color is named for
 * screen readers, the emoji is read as part of the text. `tooltip` off leaves the dot without a
 * tooltip (session rows have none).
 */
export function SessionMarker({
  marker,
  tooltip = true,
}: {
  marker: Marker | undefined;
  tooltip?: boolean;
}) {
  const t = useT();
  if (!hasMarker(marker)) return null;
  const colorText = markerColorText(marker, t);
  return (
    <span className="session-marker">
      {marker.color !== null && (
        <span
          className="marker-dot"
          style={{ background: marker.color }}
          title={tooltip ? colorText : undefined}
        >
          <span className="sr-only">{colorText}</span>
        </span>
      )}
      {marker.emoji !== null && <span className="marker-emoji">{marker.emoji}</span>}
    </span>
  );
}
