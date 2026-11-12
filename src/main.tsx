import "@xterm/xterm/css/xterm.css";
import "./fonts.css";
import "./styles.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { FileViewerWindow } from "./FileViewerWindow";
import { NotesWindow } from "./NotesWindow";
import { detectLanguage } from "./i18n";
import { loadSettings } from "./settings";
import { applyDocumentTheme, resolveTheme, systemPrefersDark } from "./theme";

// Before the first render, so a light window does not briefly flash dark (and vice versa).
applyDocumentTheme(
  resolveTheme(loadSettings(detectLanguage(navigator.language)).theme, systemPrefersDark()),
);

const root = document.getElementById("root");
if (!root) throw new Error("Element #root missing in index.html");

// The backend opens the detached notes window with `?window=notes` (notes_window.rs) and file
// viewers with `?window=file` (file_viewer.rs).
const kind = new URLSearchParams(window.location.search).get("window");

createRoot(root).render(
  <StrictMode>
    {kind === "notes" ? <NotesWindow /> : kind === "file" ? <FileViewerWindow /> : <App />}
  </StrictMode>,
);
