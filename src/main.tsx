import "@xterm/xterm/css/xterm.css";
import "./fonts.css";
import "./styles.css";

import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";

import { watchDevicePx } from "./devicePixel";
import { detectLanguage } from "./i18n";
import { loadSettings } from "./settings";
import { applyDocumentTheme, resolveTheme, systemPrefersDark } from "./theme";

// Before the first render, so a light window does not briefly flash dark (and vice versa).
applyDocumentTheme(
  resolveTheme(loadSettings(detectLanguage(navigator.language)).theme, systemPrefersDark()),
);

// Lines on whole device pixels (tree guides), also after moving to a display with another scale.
watchDevicePx(document.documentElement);

const root = document.getElementById("root");
if (!root) throw new Error("Element #root missing in index.html");

// The backend opens the detached side panel with `?window=panel` (panel_window.rs) and file
// viewers with `?window=file` (file_viewer.rs). Each window loads only its own code: a file
// viewer does not parse the whole app.
const kind = new URLSearchParams(window.location.search).get("window");

const WindowView =
  kind === "panel"
    ? lazy(() => import("./PanelWindow").then((module) => ({ default: module.PanelWindow })))
    : kind === "file"
      ? lazy(() =>
          import("./FileViewerWindow").then((module) => ({ default: module.FileViewerWindow })),
        )
      : lazy(() => import("./App").then((module) => ({ default: module.App })));

createRoot(root).render(
  <StrictMode>
    {/* The window's background is already themed; nothing to show for the moment it loads. */}
    <Suspense fallback={null}>
      <WindowView />
    </Suspense>
  </StrictMode>,
);
