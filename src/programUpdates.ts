// New versions of the installed programs (check_program_updates). The response arrives as
// `unknown`; only complete, plausible entries get through.

export interface ProgramUpdate {
  installed: string | null;
  latest: string | null;
  updateAvailable: boolean;
  /** Command for updating, for display and copying. */
  updateHint: string | null;
  /**
   * The backend can launch the update as a session (`createUpdateSession`). Otherwise
   * `updateHint` is often just the install command and remains for copying.
   */
  canRun: boolean;
  /** Unix time in ms of the query that `latest` comes from. */
  checkedAt: number;
}

export type ProgramUpdates = ReadonlyMap<string, ProgramUpdate>;

export const NO_UPDATES: ProgramUpdates = new Map();

/** Every 6 hours; the backend keeps the results in its disk cache for 24 h anyway. */
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const MAX_VERSION_LENGTH = 40;
const MAX_HINT_LENGTH = 500;

function shortText(value: unknown, max: number): string | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  return text.length > 0 && text.length <= max ? text : null;
}

function update(value: unknown): ProgramUpdate | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const field = (name: string): unknown => (name in value ? Reflect.get(value, name) : undefined);
  const available = field("updateAvailable");
  const checkedAt = field("checkedAt");
  if (typeof available !== "boolean") return null;
  if (typeof checkedAt !== "number" || !Number.isFinite(checkedAt)) return null;
  const latest = shortText(field("latest"), MAX_VERSION_LENGTH);
  return {
    installed: shortText(field("installed"), MAX_VERSION_LENGTH),
    latest,
    // Without a known new version there is nothing to show, even if the field says otherwise.
    updateAvailable: available && latest !== null,
    updateHint: shortText(field("updateHint"), MAX_HINT_LENGTH),
    // Only an explicit `true` counts; older backends do not know the field.
    canRun: field("canRun") === true,
    checkedAt,
  };
}

export function parseProgramUpdates(value: unknown): ProgramUpdates {
  const result = new Map<string, ProgramUpdate>();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return result;
  for (const [id, entry] of Object.entries(value)) {
    const parsed = update(entry);
    if (parsed) result.set(id, parsed);
  }
  return result;
}

/** An update is available and Hangar can launch it itself. */
export function runnableUpdate(updates: ProgramUpdates, profileId: string): boolean {
  const entry = updates.get(profileId);
  return entry !== undefined && entry.updateAvailable && entry.canRun;
}

/** New version if an update is available, otherwise `null`. */
export function availableVersion(updates: ProgramUpdates, profileId: string): string | null {
  const entry = updates.get(profileId);
  return entry?.updateAvailable ? entry.latest : null;
}
