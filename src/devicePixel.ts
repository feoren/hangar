// One device pixel in CSS pixels, for lines that must land on whole device pixels: at 125 % or
// 150 % display scale one CSS pixel is 1.25 or 1.5 device pixels, and a 1px dot would smear over
// two of them. The app sets --device-px on the root and updates it when the window moves to a
// display with another scale (or the zoom changes); styles.css derives the tree guide's dot and
// the row edge's drawn width from it with round(), and keeps 1px as the fallback.

/** The CSS length of one device pixel; one CSS pixel for a broken ratio. */
export function devicePx(devicePixelRatio: number): number {
  return Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? 1 / devicePixelRatio : 1;
}

/** The property's value, short but precise enough to stay on the device pixel. */
export function devicePxValue(devicePixelRatio: number): string {
  return `${String(Math.round(devicePx(devicePixelRatio) * 1e6) / 1e6)}px`;
}

/** Keeps --device-px on `root` current; returns the function that stops watching. */
export function watchDevicePx(root: HTMLElement): () => void {
  let query: MediaQueryList | null = null;
  const update = (): void => {
    query?.removeEventListener("change", update);
    const ratio = window.devicePixelRatio;
    root.style.setProperty("--device-px", devicePxValue(ratio));
    // Fires once the ratio is no longer this one; then the next ratio gets its own query.
    query = window.matchMedia(`(resolution: ${String(ratio)}dppx)`);
    query.addEventListener("change", update);
  };
  update();
  return () => {
    query?.removeEventListener("change", update);
  };
}
