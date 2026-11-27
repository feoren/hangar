// The local model servers' state (ADR 0022), outside React state like the power store
// (usePower.ts): the backend probes Ollama and LM Studio on its own and sends `local-llm-changed`
// only on a change. The window reads the state once when it starts and asks for a probe at once
// whenever it gains focus or becomes visible again, so a server started a moment ago shows up
// without waiting for the next round.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { api, LOCAL_LLM_CHANGED } from "./api";
import { LocalLlmStore, type LocalLlmState } from "./localLlm";

const store = new LocalLlmStore();
let started = false;
let refreshing = false;

function read(): void {
  api
    .localLlmStatus()
    .then((value) => {
      store.accept(value);
    })
    .catch(() => undefined);
}

/** Focus and visibility often fire together: one probe at a time is enough. */
function refresh(): void {
  if (refreshing) return;
  refreshing = true;
  api
    .localLlmRefresh()
    .then((value) => {
      store.accept(value);
    })
    .catch(() => undefined)
    .finally(() => {
      refreshing = false;
    });
}

function refreshWhenVisible(): void {
  if (document.visibilityState === "visible") refresh();
}

/** Starts listening once; outside Tauri (browser preview) the state stays unknown (`null`). */
export function startLocalLlmWatch(): void {
  if (started || typeof document === "undefined") return;
  started = true;
  try {
    listen(LOCAL_LLM_CHANGED, (event) => {
      store.accept(event.payload);
    }).catch(() => undefined);
    read();
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refreshWhenVisible);
  } catch {
    // No backend events outside Tauri.
  }
}

function subscribe(listener: () => void): () => void {
  startLocalLlmWatch();
  return store.subscribe(listener);
}

/** Ollama's and LM Studio's state; `null` until the backend first answered. */
export function useLocalLlm(): LocalLlmState | null {
  return useSyncExternalStore(subscribe, () => store.state());
}
