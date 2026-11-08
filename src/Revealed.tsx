import { invisibleSegments } from "./invisible";

/**
 * Text from outside (git) as text nodes, with invisible characters shown as marked placeholders
 * such as `<U+202E>`. Never HTML.
 */
export function Revealed({ text }: { text: string }) {
  const segments = invisibleSegments(text);
  if (segments.length === 1 && segments[0]?.hidden === false) return <>{text}</>;
  return (
    <>
      {segments.map((segment, index) =>
        segment.hidden ? (
          // Segments never move; the index is stable for this text.
          <span key={index} className="invisible-char">
            {segment.text}
          </span>
        ) : (
          <span key={index}>{segment.text}</span>
        ),
      )}
    </>
  );
}
