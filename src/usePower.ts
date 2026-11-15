// The power saving store, outside React state like the system bar (useSystemUsage.ts): the backend
// sends `power-changed` only on a change, and the window reads the state afresh when it starts,
// gains focus or becomes visible again. The pollers subscribe through `onPowerSavingChange`; the
// root element carries SAVING_CLASS while saving, so the CSS stills the row pulses.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { api, POWER_CHANGED } from "./api";
import { PowerStore, SAVING_CLASS, type PowerState } from "./power";

const store = new PowerStore();
let started = false;

function read(): void {
  api
    .powerState()
    .then((value) => {
      store.accept(value);
    })
    .catch(() => undefined);
}

function refreshWhenVisible(): void {
  if (document.visibilityState === "visible") read();
}

function applyClass(): void {
  document.documentElement.classList.toggle(SAVING_CLASS, store.saving());
}

/** Starts listening once; outside Tauri (browser preview) the state stays unknown. */
export function startPowerWatch(): void {
  if (started || typeof document === "undefined") return;
  started = true;
  store.subscribe(applyClass);
  try {
    listen(POWER_CHANGED, (event) => {
      store.accept(event.payload);
    }).catch(() => undefined);
    read();
    window.addEventListener("focus", read);
    document.addEventListener("visibilitychange", refreshWhenVisible);
  } catch {
    // No backend events outside Tauri.
  }
}

/** The setting "save energy on battery". */
export function setPowerSavingEnabled(on: boolean): void {
  store.setEnabled(on);
}

/** Whether saving is on right now, for the pollers. */
export function isPowerSaving(): boolean {
  return store.saving();
}

/** Calls `listener` on every change of the state or the setting; returns the unsubscribe. */
export function onPowerSavingChange(listener: () => void): () => void {
  return store.subscribe(listener);
}

function subscribe(listener: () => void): () => void {
  startPowerWatch();
  return store.subscribe(listener);
}

export function usePowerState(): PowerState {
  return useSyncExternalStore(subscribe, () => store.state());
}

export function usePowerSaving(): boolean {
  return useSyncExternalStore(subscribe, () => store.saving());
}
