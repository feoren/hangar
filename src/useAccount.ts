// One account store for the whole window: Settings (Account) and the sidebar (Team) read the same
// state, so the backend is asked once and its `account-changed` events are heard once.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useSyncExternalStore } from "react";

import {
  avatarSource,
  avatarMarker,
  isCurrentView,
  nextAvatarFetch,
  lacksUser,
  parseAccountError,
  parseAccountView,
  parseProjects,
  projectsAction,
  USER_RECHECK_MS,
  type AccountPage,
  type AccountProblem,
  type AccountView,
  type Project,
} from "./account";
import { ACCOUNT_CHANGED, api } from "./api";

export interface AccountSnapshot {
  /** `null` until the backend answered (or outside Tauri). */
  view: AccountView | null;
  /** The first status could not be read; `view` stays `null`. */
  unavailable: boolean;
  /** `null` while not loaded. */
  projects: Project[] | null;
  projectsError: AccountProblem | null;
  loadingProjects: boolean;
  /** The signed-in person's picture as a checked `data:` URL; `null` shows initials. */
  avatar: string | null;
}

let snapshot: AccountSnapshot = {
  view: null,
  unavailable: false,
  projects: null,
  projectsError: null,
  loadingProjects: false,
  avatar: null,
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
  // Older than what is on screen: an answer overtaken by an event. Still a valid answer.
  if (!isCurrentView(snapshot.view, view)) return snapshot.view;
  const action = projectsAction(snapshot.view, view);
  update({ view, unavailable: false });
  if (action === "load") void loadProjects();
  if (action === "clear") update({ projects: null, projectsError: null });
  scheduleUserRecheck();
  syncAvatar();
  return view;
}

/** The picture version asked for last; `null` when none is wanted or the request failed. */
let avatarRequested: number | null = null;

/** Fetches the picture when the view's marker changed, and drops it when there is none. */
function syncAvatar(): void {
  const version = nextAvatarFetch(avatarRequested, snapshot.view);
  if (version === null) {
    if (avatarMarker(snapshot.view) === null) {
      avatarRequested = null;
      if (snapshot.avatar !== null) update({ avatar: null });
    }
    return;
  }
  avatarRequested = version;
  api
    .accountAvatar()
    .then((value) => {
      // A newer version may have been asked for meanwhile; only the current answer counts.
      if (avatarRequested === version) update({ avatar: avatarSource(value) });
    })
    .catch(() => {
      if (avatarRequested === version) avatarRequested = null;
    });
}

let recheck: ReturnType<typeof setTimeout> | null = null;
let rechecks = 0;

/**
 * While signed in without the person's details, asks the backend again a few times (each status
 * starts another load of `/me` there); stops once they arrived or the sign-in ended.
 */
function scheduleUserRecheck(): void {
  if (!lacksUser(snapshot.view)) {
    if (recheck !== null) clearTimeout(recheck);
    recheck = null;
    rechecks = 0;
    return;
  }
  const pause = USER_RECHECK_MS[rechecks];
  if (recheck !== null || pause === undefined) return;
  rechecks += 1;
  recheck = setTimeout(() => {
    recheck = null;
    api
      .accountStatus()
      .then(accept)
      .catch(() => undefined);
  }, pause);
}

async function listenForChanges(): Promise<void> {
  try {
    await listen(ACCOUNT_CHANGED, (event) => {
      accept(event.payload);
    });
  } catch {
    // Outside Tauri (browser preview) there are no backend events.
  }
}

function start(): void {
  if (started) return;
  started = true;
  // Listening first: a change published between the status answer and the listener would
  // otherwise be lost, and the window would keep the older state.
  void listenForChanges().then(() =>
    api
      .accountStatus()
      .then((value) => {
        if (accept(value) === null) update({ unavailable: true });
      })
      .catch(() => {
        update({ unavailable: true });
      }),
  );
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

/** The signed-in person's picture for an `Avatar`; `null` shows initials. */
export function useAccountAvatar(): string | null {
  return useAccount().avatar;
}

export function useAccount(): AccountSnapshot {
  useEffect(start, []);
  return useSyncExternalStore(subscribe, current);
}
