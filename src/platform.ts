// The desktop platform the window runs on, from the webview's user agent (WebView2 on Windows,
// WKWebView on macOS, WebKitGTK on Linux), and what Hangar can offer there. Features the platform
// lacks are hidden, not shown without effect (ADR 0013).

import type { MessageKey } from "./i18n";

export type Platform = "windows" | "mac" | "linux";

export function platformOf(userAgent: string): Platform {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Mac/i.test(userAgent)) return "mac";
  return "linux";
}

/** The platform of this window; outside a browser (tests without a DOM) Linux. */
export function currentPlatform(): Platform {
  return platformOf(typeof navigator === "undefined" ? "" : navigator.userAgent);
}

/** GPU load and memory: only Windows' performance counters deliver them (`gpu.rs`). */
export function hasGpuReading(platform: Platform): boolean {
  return platform === "windows";
}

/** Desktop notifications: Windows toasts and the freedesktop service on Linux (`notify.rs`). */
export function hasDesktopNotifications(platform: Platform): boolean {
  return platform !== "mac";
}

/** "Show in Explorer", "Show in Finder" or "Show in the file manager". */
export function revealLabelKey(platform: Platform): MessageKey {
  if (platform === "windows") return "notes.revealExplorer";
  if (platform === "mac") return "notes.revealFinder";
  return "notes.revealFileManager";
}
