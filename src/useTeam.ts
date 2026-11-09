// One team store for the whole window, like useAccount.ts: the sidebar's Team group reads it, the
// backend's `team-changed` events are heard once. Every command result is checked in team.ts.

import { listen } from "@tauri-apps/api/event";
import { useEffect, useSyncExternalStore } from "react";

import { TEAM_CHANGED, api } from "./api";
import {
  parsePendingConfirmations,
  parseTeamError,
  parseTeamSnapshot,
  parseTeamStatus,
  statusesOf,
  withoutRequest,
  type PendingConfirmation,
  type TeamProblem,
  type TeamStatus,
} from "./team";

export interface TeamStore {
  /** Projects missing here are off. */
  statuses: ReadonlyMap<string, TeamStatus>;
  pending: PendingConfirmation[];
  /** Project ids and request ids with a command on its way. */
  busy: ReadonlySet<string>;
}

let store: TeamStore = { statuses: new Map(), pending: [], busy: new Set() };
const listeners = new Set<() => void>();
let started = false;
/** Grows with every `team-changed`; an answer asked for before a newer event is dropped. */
let generation = 0;

const BAD_RESPONSE: TeamProblem = { kind: "unknown", detail: null };

function update(patch: Partial<TeamStore>): void {
  store = { ...store, ...patch };
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function current(): TeamStore {
  return store;
}

function setStatus(projectId: string, status: TeamStatus): void {
  const statuses = new Map(store.statuses);
  statuses.set(projectId, status);
  update({ statuses });
}

function setBusy(id: string, busy: boolean): void {
  const next = new Set(store.busy);
  if (busy) next.add(id);
  else next.delete(id);
  update({ busy: next });
}

function start(): void {
  if (started) return;
  started = true;
  try {
    listen(TEAM_CHANGED, (event) => {
      const snapshot = parseTeamSnapshot(event.payload);
      if (snapshot === null) return;
      generation += 1;
      update({ statuses: statusesOf(snapshot), pending: snapshot.pending });
    }).catch(() => undefined);
  } catch {
    // Outside Tauri (browser preview) there are no backend events.
  }
  const asked = generation;
  api
    .teamPendingConfirmations()
    .then((value) => {
      const pending = parsePendingConfirmations(value);
      if (pending !== null && asked === generation) update({ pending });
    })
    .catch(() => undefined);
}

/** Reads the status of the given projects; an event that arrived meanwhile wins. */
export function loadTeamStatuses(projectIds: readonly string[]): void {
  const asked = generation;
  for (const projectId of projectIds) {
    api
      .teamStatus(projectId)
      .then((value) => {
        const status = parseTeamStatus(value);
        if (status !== null && asked === generation) setStatus(projectId, status);
      })
      .catch(() => undefined);
  }
}

/** Runs a command for `id`; resolves with the problem when it was rejected, else `null`. */
async function command(id: string, run: () => Promise<boolean>): Promise<TeamProblem | null> {
  // The buttons are disabled meanwhile; this only guards a double click within one frame.
  if (store.busy.has(id)) return { kind: "busy", detail: null };
  setBusy(id, true);
  try {
    return (await run()) ? null : BAD_RESPONSE;
  } catch (error: unknown) {
    return parseTeamError(error);
  } finally {
    setBusy(id, false);
  }
}

function toggle(projectId: string, on: boolean): Promise<TeamProblem | null> {
  return command(projectId, async () => {
    const status = parseTeamStatus(
      await (on ? api.teamEnable(projectId) : api.teamDisable(projectId)),
    );
    if (status === null) return false;
    setStatus(projectId, status);
    return true;
  });
}

async function answer(request: string, confirm: boolean): Promise<TeamProblem | null> {
  const problem = await command(request, async () => {
    await (confirm ? api.teamConfirm(request) : api.teamReject(request));
    return true;
  });
  // Answered, or already gone on the backend's side: either way it leaves the list.
  if (problem === null || problem.kind === "unknownRequest") {
    update({ pending: withoutRequest(store.pending, request) });
  }
  return problem;
}

export const team = {
  setSharing: toggle,
  confirm: (request: string) => answer(request, true),
  reject: (request: string) => answer(request, false),
};

export function useTeam(): TeamStore {
  useEffect(start, []);
  return useSyncExternalStore(subscribe, current);
}
