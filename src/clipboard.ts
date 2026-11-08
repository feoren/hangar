// Clipboard for menu actions. `navigator.clipboard` fails in WebView2 outside of a native paste
// event, so the backend's clipboard plugin does the work. In the browser preview (no Tauri) the
// web API is the fallback.

import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function readClipboard(): Promise<string> {
  try {
    return await readText();
  } catch {
    return navigator.clipboard.readText();
  }
}

export async function writeClipboard(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch {
    await navigator.clipboard.writeText(text);
  }
}
