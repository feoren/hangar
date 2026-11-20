// Rendering engine of the webview. Windows runs WebView2 (Chromium), macOS WKWebView and Linux
// WebKitGTK. Form controls and scrollbars are drawn by the engine, so the WebKit ones get their own
// styles (webkit.css) that redraw them as Chromium draws them; Chromium keeps its native controls.

export type Engine = "chromium" | "webkit";

/**
 * WebKit when the user agent names AppleWebKit without a Chromium token. Chromium based browsers
 * (WebView2, Chrome, Edge) also carry "AppleWebKit" for compatibility, but always "Chrome/" too.
 */
export function engineFromUserAgent(userAgent: string): Engine {
  const webKit = userAgent.includes("AppleWebKit/");
  const chromium = /\b(Chrome|Chromium|Edg|HeadlessChrome)\//.test(userAgent);
  return webKit && !chromium ? "webkit" : "chromium";
}

/** Marks the document (`data-engine`) before the first render, so no control redraws visibly. */
export function applyDocumentEngine(root: Pick<HTMLElement, "dataset">, userAgent: string): Engine {
  const engine = engineFromUserAgent(userAgent);
  root.dataset.engine = engine;
  return engine;
}
