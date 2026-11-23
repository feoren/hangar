// Who gets the team functions (owner's decision, 2026-11-23): the Free plan is for one person, so
// team sessions, sharing and invitations need a team to be in. The server enforces every limit;
// this only decides what the window offers, so nobody is shown controls that cannot work.

import type { AccountView, Me, Project } from "./account";
import type { TeamStatus } from "./team";

/** Plans and licenses are bought here, also for a self-hosted server. */
export const PRICING_URL = "https://hangar.feoren.com/pricing";

/**
 * `yes`: offer team functions. `no`: explain the Team plan instead. `pending`: signed in, but the
 * person's seats or projects are not known yet (loading or a network failure); the window keeps
 * offering them, so a member of a paid team is never locked out by a slow answer.
 */
export type TeamAccess = "yes" | "no" | "pending";

/** More than one seat: a paid plan or license. Free has one seat, the person alone. */
function paidSeats(seats: Pick<Me, "seatsAccountTotal">): boolean {
  return (seats.seatsAccountTotal ?? 0) > 1;
}

/**
 * A project the person really shares with others: someone else's (the person was let in), or an
 * own one that has room for or already holds more than one person (paid, or grandfathered from
 * when Free allowed three).
 */
export function isTeamProject(project: Project): boolean {
  if (project.role !== "owner") return true;
  return paidSeats(project) || (project.seatsAccountUsed ?? 0) > 1;
}

/**
 * Team functions are offered to a person in at least one team, or whose own account has more
 * than one seat. Signed out, or signed in on Free without a team: `no`.
 */
export function teamAccess(
  view: AccountView | null,
  projects: readonly Project[] | null,
): TeamAccess {
  if (view === null) return "pending";
  if (view.state.kind !== "signedIn") return "no";
  const user = view.state.user;
  if (user !== null && paidSeats(user)) return "yes";
  if (projects?.some(isTeamProject) === true) return "yes";
  if (user === null || projects === null) return "pending";
  return "no";
}

/** Whether team functions are offered now; `pending` keeps them, see `TeamAccess`. */
export function offersTeam(access: TeamAccess): boolean {
  return access !== "no";
}

/** Every seat of the owner's account is taken; `false` when the server sent no numbers. */
export function seatsFull(seats: Pick<Project, "seatsAccountTotal" | "seatsAccountUsed">): boolean {
  const { seatsAccountTotal: total, seatsAccountUsed: used } = seats;
  return total !== null && used !== null && used >= total;
}

/**
 * The team server refused this device while every seat of the project's plan is taken: most
 * likely there is no seat for it, so the window names the plan next to the bare "refused".
 */
export function refusedForSeat(project: Project, status: TeamStatus): boolean {
  const refused = status.state === "needsAttention" && status.reason === "refused";
  return refused && seatsFull(project);
}

/**
 * The project has no seat for anyone new: an invitation would be rejected, or this device was
 * refused for want of one. The window then names the Team plan instead of offering "Invite
 * people" or leaving a bare "refused".
 */
export function noSeatHint(project: Project, status: TeamStatus): boolean {
  return (project.canInvite && seatsFull(project)) || refusedForSeat(project, status);
}

/** Free with one seat of its own: Settings names the plan instead of "1 of 1 seats". */
export function onFreePlan(user: Me): boolean {
  return user.seatsAccountTotal !== null && user.seatsAccountTotal <= 1;
}

/**
 * The support link (Buy me a coffee) is for people on Free: signed out, on Free or while the plan
 * is not known yet it shows; signed in with a paid plan (more than one seat) it stays away.
 */
export function showsSupportLink(view: AccountView | null): boolean {
  if (view?.state.kind !== "signedIn") return true;
  const user = view.state.user;
  return user === null || !paidSeats(user);
}
