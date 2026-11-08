import "@xterm/xterm/css/xterm.css";
import "./styles.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { detectLanguage } from "./i18n";
import { loadSettings } from "./settings";
import { applyDocumentTheme, resolveTheme, systemPrefersDark } from "./theme";

// Before the first render, so a light window does not briefly flash dark (and vice versa).
applyDocumentTheme(
  resolveTheme(loadSettings(detectLanguage(navigator.language)).theme, systemPrefersDark()),
);

const root = document.getElementById("root");
if (!root) throw new Error("Element #root missing in index.html");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
