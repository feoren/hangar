// One account store for the whole window: Settings (Account) and the sidebar (Team) read the same
// state, so the backend is asked once and its `account-changed` events are heard once.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useSyncExternalStore } from "react";

import {
  parseAccountError,
  parseAccountView,
  parseProjects,
  projectsAction,
  type AccountPage,
  type AccountProblem,
  type AccountView,
  type Project,
} from "./account";
import { ACCOUNT_CHANGED, api } from "./api";

export interface AccountSnapshot {
  /** `null` until the backend answered (or outside Tauri). */
  view: AccountView | null;
  /** `null` while not loaded. */
  projects: Project[] | null;
  projectsError: AccountProblem | null;
  loadingProjects: boolean;
}

let snapshot: AccountSnapshot = {
  view: null,
  projects: null,
  projectsError: null,
  loadingProjects: false,
};
const listeners = new Set<() => void>();
let started = false;

function update(patch: Partial<AccountSnapshot>): void {
  snapshot = { ...snapshot, ...patch };
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function current(): AccountSnapshot {
  return snapshot;
}

function accept(value: unknown): AccountView | null {
  const view = parseAccountView(value);
  if (view === null) return null;
  const action = projectsAction(snapshot.view, view);
  update({ view });
  if (action === "load") void loadProjects();
  if (action === "clear") update({ projects: null, projectsError: null });
  return view;
}

function start(): void {
  if (started) return;
  started = true;
  try {
    listen(ACCOUNT_CHANGED, (event) => {
      accept(event.payload);
    }).catch(() => undefined);
  } catch {
    // Outside Tauri (browser preview) there are no backend events.
  }
  api
    .accountStatus()
    .then(accept)
    .catch(() => undefined);
}

/** Runs an account command; resolves with the problem when it was rejected, else `null`. */
async function command(run: () => Promise<unknown>): Promise<AccountProblem | null> {
  try {
    accept(await run());
    return null;
  } catch (error: unknown) {
    return parseAccountError(error);
  }
}

export async function loadProjects(): Promise<void> {
  update({ loadingProjects: true });
  try {
    const projects = parseProjects(await api.accountProjects());
    update(
      projects === null
        ? { projectsError: { kind: "badResponse", detail: null } }
        : { projects, projectsError: null },
    );
  } catch (error: unknown) {
    update({ projectsError: parseAccountError(error) });
  } finally {
    update({ loadingProjects: false });
  }
}

export const account = {
  signIn: () => command(() => api.accountSignIn()),
  cancelSignIn: () => command(() => api.accountCancelSignIn()),
  signOut: () => command(() => api.accountSignOut()),
  setBaseUrl: (baseUrl: string | null) => command(() => api.accountSetBaseUrl(baseUrl)),
  open: async (page: AccountPage): Promise<AccountProblem | null> => {
    try {
      await api.accountOpen(page);
      return null;
    } catch (error: unknown) {
      return parseAccountError(error);
    }
  },
};

export function useAccount(): AccountSnapshot {
  useEffect(start, []);
  return useSyncExternalStore(subscribe, current);
}
