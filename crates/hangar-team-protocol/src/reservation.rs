//! Task reservations (ADR 0005): whoever reserves first holds the task; a hold expires after
//! inactivity while its holder is offline, with a warning first; a heartbeat (the agent changed
//! files) extends it; another member can ask for a release, which the holder accepts or declines
//! (or which times out, or which the requester withdraws); an owner or admin can end any hold.
//!
//! Pure state machines with injected time. The server owns one [`ReservationBook`] per team and
//! calls [`ReservationBook::apply`] for every intent in arrival order: that call is the
//! authority that decides "first wins". It never applies anything optimistically on a client.
//!
//! Time rules (all with `now` from the server's clock):
//! - The deadline of a hold is `last_activity + inactivity`. While the holder is online it does
//!   not run, unless [`Policy::expire_while_online`] is set. When the holder goes offline the
//!   deadline is at least `offline_since + warn_before`, so a warning always comes first.
//! - At `deadline - warn_before` (inclusive) the hold turns `Expiring` and the holder is warned
//!   once; at `deadline` (inclusive) it is `Expired` and anyone may reserve the task.
//! - A release request ends at `asked + request_timeout` (inclusive) unless the hold ends first.
//! - Time is only looked at when something happens (an intent, presence, or
//!   [`ReservationBook::tick`]); the server ticks at [`ReservationBook::next_wakeup`]. A tick
//!   that comes later than the deadline expires the hold without a warning.
//! - Events that time caused are returned apart from the intent's outcome ([`Applied::timing`]),
//!   so they reach the team even when the intent is refused, and exactly once.

use std::collections::HashMap;

use thiserror::Error;

use crate::hash::KeyedHash;
use crate::ids::{Epoch, MemberId, UnixMillis};
use crate::limits::MAX_RESERVATIONS_PER_MEMBER;
use crate::wire::{ErrorCode, Holder, ReleaseReason, RequestEndReason, Role};

const MINUTE: u64 = 60_000;

/// Timing of reservations; per team on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    inactivity_ms: u64,
    warn_before_ms: u64,
    expire_while_online: bool,
    request_timeout_ms: u64,
    request_cooldown_ms: u64,
    rekey_grace_ms: u64,
}

/// A [`Policy`] that cannot work.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolicyError {
    #[error("the inactivity before expiry must be at least one minute")]
    InactivityTooShort,
    #[error("the warning must come at least a minute before expiry and after the hold starts")]
    WarningOutOfRange,
    #[error("a release request must stay open for at least one minute")]
    RequestTimeoutTooShort,
    #[error("the grace period for rekeying must be at least one minute")]
    GraceTooShort,
}

impl Policy {
    /// `inactivity_ms`: time without activity before a hold expires; `warn_before_ms`: how long
    /// before that the holder is warned. Release requests and rekeying keep their defaults.
    pub fn new(
        inactivity_ms: u64,
        warn_before_ms: u64,
        expire_while_online: bool,
    ) -> Result<Self, PolicyError> {
        if inactivity_ms < MINUTE {
            return Err(PolicyError::InactivityTooShort);
        }
        if warn_before_ms < MINUTE || warn_before_ms >= inactivity_ms {
            return Err(PolicyError::WarningOutOfRange);
        }
        Ok(Self {
            inactivity_ms,
            warn_before_ms,
            expire_while_online,
            ..Self::default()
        })
    }

    /// How long a release request waits for an answer, and how long the requester then waits
    /// before asking for the same task again (after a decline, a timeout or a withdrawal), so a
    /// holder cannot be flooded with requests.
    pub fn with_requests(self, timeout_ms: u64, cooldown_ms: u64) -> Result<Self, PolicyError> {
        if timeout_ms < MINUTE {
            return Err(PolicyError::RequestTimeoutTooShort);
        }
        Ok(Self {
            request_timeout_ms: timeout_ms,
            request_cooldown_ms: cooldown_ms,
            ..self
        })
    }

    /// How long after a key rotation holds of earlier epochs stand without a rekey.
    pub fn with_rekey_grace(self, grace_ms: u64) -> Result<Self, PolicyError> {
        if grace_ms < MINUTE {
            return Err(PolicyError::GraceTooShort);
        }
        Ok(Self {
            rekey_grace_ms: grace_ms,
            ..self
        })
    }

    /// Time without activity before a hold expires.
    pub fn inactivity_ms(&self) -> u64 {
        self.inactivity_ms
    }

    /// How long before expiry the holder is warned.
    pub fn warn_before_ms(&self) -> u64 {
        self.warn_before_ms
    }

    /// Whether holds also run out while their holder is online.
    pub fn expire_while_online(&self) -> bool {
        self.expire_while_online
    }

    /// How long a release request stays open without an answer.
    pub fn request_timeout_ms(&self) -> u64 {
        self.request_timeout_ms
    }

    /// How long a requester waits before asking for the same task again.
    pub fn request_cooldown_ms(&self) -> u64 {
        self.request_cooldown_ms
    }

    /// How long holds of earlier epochs stand after a key rotation.
    pub fn rekey_grace_ms(&self) -> u64 {
        self.rekey_grace_ms
    }
}

impl Default for Policy {
    /// ADR 0005: 30 minutes without changes while the person is offline, warned 5 minutes
    /// before. A release request waits 10 minutes (an online holder sees it at once; an offline
    /// one loses the hold within 30 minutes anyway) and may be repeated 10 minutes after it
    /// ended. Holds of an old epoch stand for 30 minutes after a rotation, as long as an offline
    /// hold would.
    fn default() -> Self {
        Self {
            inactivity_ms: 30 * MINUTE,
            warn_before_ms: 5 * MINUTE,
            expire_while_online: false,
            request_timeout_ms: 10 * MINUTE,
            request_cooldown_ms: 10 * MINUTE,
            rekey_grace_ms: 30 * MINUTE,
        }
    }
}

/// One member's device holding a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub holder: Holder,
    /// The epoch of the key the task's hash was made with.
    pub epoch: Epoch,
    pub since: UnixMillis,
    pub last_activity: UnixMillis,
    /// `None` while the holder's device is connected.
    pub offline_since: Option<UnixMillis>,
}

impl Hold {
    fn new(holder: Holder, epoch: Epoch, now: UnixMillis, online: bool) -> Self {
        Self {
            holder,
            epoch,
            since: now,
            last_activity: now,
            offline_since: (!online).then_some(now),
        }
    }

    /// When the hold expires; `None` while it cannot (holder online under the default policy).
    pub fn deadline(&self, policy: &Policy) -> Option<UnixMillis> {
        let idle = self.last_activity.plus(policy.inactivity_ms);
        match self.offline_since {
            Some(offline) => Some(idle.max(offline.plus(policy.warn_before_ms))),
            None if policy.expire_while_online => Some(idle),
            None => None,
        }
    }
}

/// Someone asked the holder to hand the task over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRequest {
    pub requester: Holder,
    pub at: UnixMillis,
}

/// The state of one task's reservation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReservationState {
    #[default]
    Free,
    Held {
        hold: Hold,
    },
    /// The holder was warned at `warned_at`.
    Expiring {
        hold: Hold,
        warned_at: UnixMillis,
    },
    /// Another member asked for the task; the hold goes on (and may still expire) until the
    /// holder answers, the request times out or the requester withdraws it. `warned_at` is set
    /// once the expiry warning went out.
    ReleaseRequested {
        hold: Hold,
        request: ReleaseRequest,
        warned_at: Option<UnixMillis>,
    },
    /// Free again after expiry; anyone may reserve.
    Expired {
        holder: Holder,
        at: UnixMillis,
    },
}

/// What happened, for the server to log and send (each maps to one server message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservationEvent {
    /// `task.reserved`: new hold or a new deadline.
    Reserved {
        holder: Holder,
        epoch: Epoch,
        since: UnixMillis,
        expires_at: Option<UnixMillis>,
    },
    /// `task.released`.
    Released {
        holder: Holder,
        reason: ReleaseReason,
    },
    /// `reservation.expiring`.
    Expiring {
        holder: Holder,
        expires_at: UnixMillis,
    },
    /// `reservation.expired`.
    Expired { holder: Holder },
    /// `task.release_requested`.
    ReleaseRequested { holder: Holder, requester: Holder },
    /// `task.release_answered`.
    ReleaseAnswered {
        holder: Holder,
        requester: Holder,
        accepted: bool,
    },
    /// `task.release_request_ended`.
    ReleaseRequestEnded {
        holder: Holder,
        requester: Holder,
        reason: RequestEndReason,
    },
}

/// Why an intent was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Rejection {
    #[error("the task is held by someone else")]
    Conflict { holder: Holder },
    #[error("a release request is already open")]
    RequestPending,
    #[error("only the holder may do this")]
    NotHolder,
    #[error("the task is not held")]
    NotHeld,
    #[error("the holder cannot ask itself for a release")]
    OwnReservation,
    #[error("no such release request")]
    NoRequest,
    #[error("the hash was made with another epoch's key")]
    StaleEpoch,
    #[error("too many reservations")]
    Limit,
    #[error("asked for this task too recently")]
    Cooldown { until: UnixMillis },
    #[error("the old and the new hash are the same")]
    SameTask,
    #[error("the hold is already under the current epoch")]
    NotOldEpoch,
    #[error("this role may not end someone else's hold")]
    NotAllowed,
}

impl Rejection {
    /// The `err` code the server answers with.
    pub fn code(&self) -> ErrorCode {
        match self {
            Rejection::Conflict { .. } | Rejection::RequestPending => ErrorCode::Conflict,
            Rejection::NotHolder | Rejection::NotAllowed => ErrorCode::Forbidden,
            Rejection::NotHeld
            | Rejection::OwnReservation
            | Rejection::NoRequest
            | Rejection::SameTask
            | Rejection::NotOldEpoch => ErrorCode::BadRequest,
            Rejection::StaleEpoch => ErrorCode::StaleEpoch,
            Rejection::Limit => ErrorCode::Limit,
            Rejection::Cooldown { .. } => ErrorCode::RateLimited,
        }
    }

    /// For `err.holder` on a conflict.
    pub fn holder(&self) -> Option<&Holder> {
        match self {
            Rejection::Conflict { holder } => Some(holder),
            _ => None,
        }
    }

    /// For `err.retryAfterMs` on a cooldown.
    pub fn retry_after_ms(&self, now: UnixMillis) -> Option<u64> {
        match self {
            Rejection::Cooldown { until } => Some(until.0.saturating_sub(now.0)),
            _ => None,
        }
    }
}

type Outcome<E> = Result<Vec<E>, Rejection>;

/// The result of an intent: what time did before it was looked at, and the intent's own
/// outcome. The server sends `timing` in any case and `outcome`'s events only when it is `Ok`
/// (with an `ack`), otherwise an `err`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied<E> {
    pub timing: Vec<E>,
    pub outcome: Outcome<E>,
}

impl<E> Applied<E> {
    /// Every event to send, in order: `timing` first, then the intent's events if it was
    /// applied. The rejection, if any, is returned beside them.
    pub fn into_events(self) -> (Vec<E>, Option<Rejection>) {
        let mut events = self.timing;
        match self.outcome {
            Ok(more) => {
                events.extend(more);
                (events, None)
            }
            Err(rejection) => (events, Some(rejection)),
        }
    }
}

/// What the book knows about the requester when a request is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handover {
    /// The requesting device is connected (its new hold has no deadline then).
    pub requester_online: bool,
    /// The requester holds fewer than [`MAX_RESERVATIONS_PER_MEMBER`] tasks.
    pub requester_has_room: bool,
}

/// One task's reservation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reservation {
    state: ReservationState,
    /// Members who may not ask for this task again before the instant. Cleared when the task
    /// gets a new holder.
    cooldowns: Vec<(MemberId, UnixMillis)>,
}

impl Reservation {
    /// The current state.
    pub fn state(&self) -> &ReservationState {
        &self.state
    }

    fn hold(&self) -> Option<&Hold> {
        hold_of(&self.state)
    }

    /// Who holds the task, if anyone.
    pub fn holder(&self) -> Option<&Holder> {
        holder_of(&self.state)
    }

    /// The next instant at which [`Reservation::advance`] would change something.
    pub fn next_wakeup(&self, policy: &Policy) -> Option<UnixMillis> {
        let hold = self.hold()?;
        let warned = match &self.state {
            ReservationState::Held { .. } => false,
            ReservationState::ReleaseRequested { warned_at, .. } => warned_at.is_some(),
            _ => true,
        };
        let hold_wakeup = hold.deadline(policy).map(|deadline| {
            if warned {
                deadline
            } else {
                deadline.minus(policy.warn_before_ms)
            }
        });
        let request_wakeup = match &self.state {
            ReservationState::ReleaseRequested { request, .. } => {
                Some(request.at.plus(policy.request_timeout_ms))
            }
            _ => None,
        };
        hold_wakeup.into_iter().chain(request_wakeup).min()
    }

    /// Applies the passing of time up to `now`: request timeout, warning and expiry. Each event
    /// comes once; a second call with the same `now` returns nothing.
    pub fn advance(&mut self, policy: &Policy, now: UnixMillis) -> Vec<ReservationEvent> {
        let mut events = Vec::new();
        let state = match std::mem::take(&mut self.state) {
            ReservationState::ReleaseRequested {
                hold,
                request,
                warned_at,
            } => {
                let timeout = request.at.plus(policy.request_timeout_ms);
                // A hold that runs out first takes the request with it.
                let before_expiry = hold.deadline(policy).is_none_or(|d| timeout < d);
                if now >= timeout && before_expiry {
                    events.push(request_ended(&hold, &request, RequestEndReason::TimedOut));
                    self.cool_down(
                        &request.requester.member,
                        timeout.plus(policy.request_cooldown_ms),
                        now,
                    );
                    without_request(hold, warned_at)
                } else {
                    ReservationState::ReleaseRequested {
                        hold,
                        request,
                        warned_at,
                    }
                }
            }
            other => other,
        };
        self.state = step_time(state, policy, now, &mut events);
        events
    }

    fn cool_down(&mut self, member: &MemberId, until: UnixMillis, now: UnixMillis) {
        self.cooldowns
            .retain(|(other, end)| other != member && *end > now);
        self.cooldowns.push((member.clone(), until));
    }

    fn cooldown(&self, member: &MemberId, now: UnixMillis) -> Option<UnixMillis> {
        self.cooldowns
            .iter()
            .find(|(other, until)| other == member && *until > now)
            .map(|(_, until)| *until)
    }

    fn timed<T>(&mut self, policy: &Policy, now: UnixMillis, op: T) -> Applied<ReservationEvent>
    where
        T: FnOnce(&mut Self) -> Outcome<ReservationEvent>,
    {
        let timing = self.advance(policy, now);
        let outcome = op(self);
        Applied { timing, outcome }
    }

    /// `task.reserve`. Free or expired: `who` holds it now. Held by `who`: counts as activity.
    /// Held by someone else: conflict, whatever the epoch.
    pub fn reserve(
        &mut self,
        policy: &Policy,
        who: &Holder,
        epoch: Epoch,
        online: bool,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| {
            r.do_reserve(policy, who, epoch, online, now)
        })
    }

    fn do_reserve(
        &mut self,
        policy: &Policy,
        who: &Holder,
        epoch: Epoch,
        online: bool,
        now: UnixMillis,
    ) -> Outcome<ReservationEvent> {
        match self.holder() {
            None => {
                let hold = Hold::new(who.clone(), epoch, now, online);
                let event = reserved(&hold, policy);
                self.state = ReservationState::Held { hold };
                self.cooldowns.clear();
                Ok(vec![event])
            }
            Some(holder) if holder == who => Ok(self.touch(policy, now)),
            Some(holder) => Err(Rejection::Conflict {
                holder: holder.clone(),
            }),
        }
    }

    /// `heartbeat`: the holder's agent was active. Expired or free: refused, the client has to
    /// reserve again (and may lose to someone who was faster).
    pub fn heartbeat(
        &mut self,
        policy: &Policy,
        who: &Holder,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| r.do_heartbeat(policy, who, now))
    }

    fn do_heartbeat(
        &mut self,
        policy: &Policy,
        who: &Holder,
        now: UnixMillis,
    ) -> Outcome<ReservationEvent> {
        match self.holder() {
            None => Err(Rejection::NotHeld),
            Some(holder) if holder != who => Err(Rejection::NotHolder),
            Some(_) => Ok(self.touch(policy, now)),
        }
    }

    /// Activity of the holder: a new deadline, an expiry warning is withdrawn.
    fn touch(&mut self, policy: &Policy, now: UnixMillis) -> Vec<ReservationEvent> {
        self.state = match std::mem::take(&mut self.state) {
            ReservationState::Held { mut hold } | ReservationState::Expiring { mut hold, .. } => {
                hold.last_activity = now;
                ReservationState::Held { hold }
            }
            ReservationState::ReleaseRequested {
                mut hold, request, ..
            } => {
                hold.last_activity = now;
                ReservationState::ReleaseRequested {
                    hold,
                    request,
                    warned_at: None,
                }
            }
            other => other,
        };
        self.hold()
            .map(|hold| reserved(hold, policy))
            .into_iter()
            .collect()
    }

    /// `task.release` by the holder. A pending release request ends with it; the requester sees
    /// `task.released` and may reserve.
    pub fn release(
        &mut self,
        policy: &Policy,
        who: &Holder,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| r.do_release(who))
    }

    fn do_release(&mut self, who: &Holder) -> Outcome<ReservationEvent> {
        let event = match self.holder() {
            None => return Err(Rejection::NotHeld),
            Some(holder) if holder != who => return Err(Rejection::NotHolder),
            Some(holder) => ReservationEvent::Released {
                holder: holder.clone(),
                reason: ReleaseReason::Released,
            },
        };
        self.state = ReservationState::Free;
        Ok(vec![event])
    }

    /// `task.force_release` by an owner or admin (`role` from the verified ticket): ends any
    /// hold, against squatting by a member who keeps a task without working on it.
    pub fn force_release(
        &mut self,
        policy: &Policy,
        role: Role,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| r.do_force_release(role))
    }

    fn do_force_release(&mut self, role: Role) -> Outcome<ReservationEvent> {
        let Some(holder) = self.holder().cloned() else {
            return Err(Rejection::NotHeld);
        };
        if role == Role::Member {
            return Err(Rejection::NotAllowed);
        }
        self.state = ReservationState::Free;
        Ok(vec![ReservationEvent::Released {
            holder,
            reason: ReleaseReason::ForceReleased,
        }])
    }

    /// `task.release_request` by someone who is not the holder. One open request at a time;
    /// refused while the requester is cooling down or holds [`MAX_RESERVATIONS_PER_MEMBER`]
    /// tasks (`has_room` false), since an accept could not hand the task over then.
    pub fn request_release(
        &mut self,
        policy: &Policy,
        requester: &Holder,
        has_room: bool,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| r.do_request(requester, has_room, now))
    }

    fn do_request(
        &mut self,
        requester: &Holder,
        has_room: bool,
        now: UnixMillis,
    ) -> Outcome<ReservationEvent> {
        let (hold, warned_at) = match &self.state {
            ReservationState::Held { hold } => (hold, None),
            ReservationState::Expiring { hold, warned_at } => (hold, Some(*warned_at)),
            ReservationState::ReleaseRequested { .. } => return Err(Rejection::RequestPending),
            ReservationState::Free | ReservationState::Expired { .. } => {
                return Err(Rejection::NotHeld)
            }
        };
        if hold.holder == *requester {
            return Err(Rejection::OwnReservation);
        }
        if let Some(until) = self.cooldown(&requester.member, now) {
            return Err(Rejection::Cooldown { until });
        }
        if !has_room {
            return Err(Rejection::Limit);
        }
        let hold = hold.clone();
        let event = ReservationEvent::ReleaseRequested {
            holder: hold.holder.clone(),
            requester: requester.clone(),
        };
        self.state = ReservationState::ReleaseRequested {
            hold,
            request: ReleaseRequest {
                requester: requester.clone(),
                at: now,
            },
            warned_at,
        };
        Ok(vec![event])
    }

    /// `task.release_withdraw` by the requesting member (from any of its devices).
    pub fn withdraw_release(
        &mut self,
        policy: &Policy,
        requester: &Holder,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| r.do_withdraw(policy, requester, now))
    }

    fn do_withdraw(
        &mut self,
        policy: &Policy,
        requester: &Holder,
        now: UnixMillis,
    ) -> Outcome<ReservationEvent> {
        match &self.state {
            ReservationState::ReleaseRequested { request, .. }
                if request.requester.member == requester.member => {}
            ReservationState::Free | ReservationState::Expired { .. } => {
                return Err(Rejection::NotHeld)
            }
            _ => return Err(Rejection::NoRequest),
        }
        let ReservationState::ReleaseRequested {
            hold,
            request,
            warned_at,
        } = std::mem::take(&mut self.state)
        else {
            return Err(Rejection::NoRequest);
        };
        let event = request_ended(&hold, &request, RequestEndReason::Withdrawn);
        self.state = without_request(hold, warned_at);
        self.cool_down(
            &request.requester.member,
            now.plus(policy.request_cooldown_ms),
            now,
        );
        Ok(vec![event])
    }

    /// `task.release_answer` by the holder. Accepted: the requester holds the task from now on
    /// (refused with `Limit` while the requester has no room; the request stays open).
    /// Declined: the hold goes on as before and the requester cools down.
    pub fn answer_release(
        &mut self,
        policy: &Policy,
        who: &Holder,
        requester: &MemberId,
        accept: bool,
        handover: Handover,
        now: UnixMillis,
    ) -> Applied<ReservationEvent> {
        self.timed(policy, now, |r| {
            r.do_answer(policy, who, requester, accept, handover, now)
        })
    }

    fn do_answer(
        &mut self,
        policy: &Policy,
        who: &Holder,
        requester: &MemberId,
        accept: bool,
        handover: Handover,
        now: UnixMillis,
    ) -> Outcome<ReservationEvent> {
        match &self.state {
            ReservationState::ReleaseRequested { hold, request, .. } => {
                if hold.holder != *who {
                    return Err(Rejection::NotHolder);
                }
                if request.requester.member != *requester {
                    return Err(Rejection::NoRequest);
                }
                if accept && !handover.requester_has_room {
                    return Err(Rejection::Limit);
                }
            }
            state => {
                return Err(match holder_of(state) {
                    Some(holder) if holder != who => Rejection::NotHolder,
                    Some(_) => Rejection::NoRequest,
                    None => Rejection::NotHeld,
                })
            }
        }
        let ReservationState::ReleaseRequested {
            hold,
            request,
            warned_at,
        } = std::mem::take(&mut self.state)
        else {
            return Err(Rejection::NoRequest);
        };
        let mut events = vec![ReservationEvent::ReleaseAnswered {
            holder: hold.holder.clone(),
            requester: request.requester.clone(),
            accepted: accept,
        }];
        if accept {
            events.push(ReservationEvent::Released {
                holder: hold.holder.clone(),
                reason: ReleaseReason::HandedOver,
            });
            let next = Hold::new(
                request.requester,
                hold.epoch,
                now,
                handover.requester_online,
            );
            events.push(reserved(&next, policy));
            self.state = ReservationState::Held { hold: next };
            self.cooldowns.clear();
        } else {
            self.state = without_request(hold, warned_at);
            self.cool_down(
                &request.requester.member,
                now.plus(policy.request_cooldown_ms),
                now,
            );
        }
        Ok(events)
    }

    /// The holder's device connected or disconnected. Time up to `now` is applied with the old
    /// presence first.
    pub fn presence(
        &mut self,
        policy: &Policy,
        who: &Holder,
        online: bool,
        now: UnixMillis,
    ) -> Vec<ReservationEvent> {
        let mut events = self.advance(policy, now);
        let hold = match &mut self.state {
            ReservationState::Held { hold }
            | ReservationState::Expiring { hold, .. }
            | ReservationState::ReleaseRequested { hold, .. } => hold,
            _ => return events,
        };
        if hold.holder != *who || hold.offline_since.is_none() == online {
            return events;
        }
        let before = hold.deadline(policy);
        hold.offline_since = (!online).then_some(now);
        if hold.deadline(policy) != before {
            events.push(reserved(hold, policy));
        }
        // Coming back online can end an `Expiring` (no deadline any more).
        events.extend(self.advance(policy, now));
        events
    }

    /// `member` left the team: a hold of theirs ends, and a release request of theirs is
    /// withdrawn without an event (the team learns of it from `member.removed`), so the task can
    /// never be handed to someone who is gone and a new request is possible. Crate-internal:
    /// it applies no time, so only [`ReservationBook::remove_member`], which does, calls it.
    pub(crate) fn remove_holder(&mut self, member: &MemberId) -> Vec<ReservationEvent> {
        match std::mem::take(&mut self.state) {
            state if holder_of(&state).is_some_and(|holder| holder.member == *member) => {
                hold_of(&state)
                    .map(|hold| ReservationEvent::Released {
                        holder: hold.holder.clone(),
                        reason: ReleaseReason::MemberRemoved,
                    })
                    .into_iter()
                    .collect()
            }
            ReservationState::ReleaseRequested {
                hold,
                request,
                warned_at,
            } if request.requester.member == *member => {
                self.state = without_request(hold, warned_at);
                Vec::new()
            }
            other => {
                self.state = other;
                Vec::new()
            }
        }
    }

    /// One device was removed (its member stays): a hold of that device ends, and a release
    /// request it made ends as withdrawn (the member may ask again from another device).
    /// Crate-internal for the same reason as `remove_holder`; see
    /// [`ReservationBook::remove_device`].
    pub(crate) fn remove_device(&mut self, device: &Holder) -> Vec<ReservationEvent> {
        match std::mem::take(&mut self.state) {
            state if holder_of(&state) == Some(device) => hold_of(&state)
                .map(|hold| ReservationEvent::Released {
                    holder: hold.holder.clone(),
                    reason: ReleaseReason::MemberRemoved,
                })
                .into_iter()
                .collect(),
            ReservationState::ReleaseRequested {
                hold,
                request,
                warned_at,
            } if request.requester == *device => {
                let event = request_ended(&hold, &request, RequestEndReason::Withdrawn);
                self.state = without_request(hold, warned_at);
                vec![event]
            }
            other => {
                self.state = other;
                Vec::new()
            }
        }
    }

    /// A hold of an old epoch that was not rekeyed in time.
    fn expire_now(&mut self, now: UnixMillis) -> Vec<ReservationEvent> {
        let mut events = Vec::new();
        if let Some(hold) = hold_of(&std::mem::take(&mut self.state)).cloned() {
            self.state = expire(hold, now, &mut events);
        }
        events
    }
}

/// Warning and expiry of a hold (the request timeout is handled before).
fn step_time(
    state: ReservationState,
    policy: &Policy,
    now: UnixMillis,
    events: &mut Vec<ReservationEvent>,
) -> ReservationState {
    match state {
        ReservationState::Held { hold } => match hold.deadline(policy) {
            Some(deadline) if now >= deadline => expire(hold, now, events),
            Some(deadline) if now >= deadline.minus(policy.warn_before_ms) => {
                events.push(ReservationEvent::Expiring {
                    holder: hold.holder.clone(),
                    expires_at: deadline,
                });
                ReservationState::Expiring {
                    hold,
                    warned_at: now,
                }
            }
            _ => ReservationState::Held { hold },
        },
        ReservationState::Expiring { hold, warned_at } => match hold.deadline(policy) {
            Some(deadline) if now >= deadline => expire(hold, now, events),
            Some(_) => ReservationState::Expiring { hold, warned_at },
            // The holder came back online: nothing runs any more.
            None => ReservationState::Held { hold },
        },
        ReservationState::ReleaseRequested {
            hold,
            request,
            warned_at,
        } => match hold.deadline(policy) {
            Some(deadline) if now >= deadline => expire(hold, now, events),
            Some(deadline)
                if warned_at.is_none() && now >= deadline.minus(policy.warn_before_ms) =>
            {
                events.push(ReservationEvent::Expiring {
                    holder: hold.holder.clone(),
                    expires_at: deadline,
                });
                ReservationState::ReleaseRequested {
                    hold,
                    request,
                    warned_at: Some(now),
                }
            }
            Some(_) => ReservationState::ReleaseRequested {
                hold,
                request,
                warned_at,
            },
            None => ReservationState::ReleaseRequested {
                hold,
                request,
                warned_at: None,
            },
        },
        other => other,
    }
}

fn without_request(hold: Hold, warned_at: Option<UnixMillis>) -> ReservationState {
    match warned_at {
        Some(warned_at) => ReservationState::Expiring { hold, warned_at },
        None => ReservationState::Held { hold },
    }
}

fn hold_of(state: &ReservationState) -> Option<&Hold> {
    match state {
        ReservationState::Held { hold }
        | ReservationState::Expiring { hold, .. }
        | ReservationState::ReleaseRequested { hold, .. } => Some(hold),
        ReservationState::Free | ReservationState::Expired { .. } => None,
    }
}

fn holder_of(state: &ReservationState) -> Option<&Holder> {
    hold_of(state).map(|hold| &hold.holder)
}

fn expire(hold: Hold, now: UnixMillis, events: &mut Vec<ReservationEvent>) -> ReservationState {
    events.push(ReservationEvent::Expired {
        holder: hold.holder.clone(),
    });
    ReservationState::Expired {
        holder: hold.holder,
        at: now,
    }
}

fn reserved(hold: &Hold, policy: &Policy) -> ReservationEvent {
    ReservationEvent::Reserved {
        holder: hold.holder.clone(),
        epoch: hold.epoch,
        since: hold.since,
        expires_at: hold.deadline(policy),
    }
}

fn request_ended(
    hold: &Hold,
    request: &ReleaseRequest,
    reason: RequestEndReason,
) -> ReservationEvent {
    ReservationEvent::ReleaseRequestEnded {
        holder: hold.holder.clone(),
        requester: request.requester.clone(),
        reason,
    }
}

/// An intent of a client that touches reservations. `task.force_release` needs the sender's
/// role and goes through [`ReservationBook::force_release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Reserve {
        task: KeyedHash,
        epoch: Epoch,
        /// The task's hashes under earlier epochs (`TaskReserve::prev`).
        prev: Vec<KeyedHash>,
    },
    Release {
        task: KeyedHash,
    },
    /// Tasks not held by the sender are skipped: the sender learns of their end through the
    /// events it already got.
    Heartbeat {
        tasks: Vec<KeyedHash>,
    },
    RequestRelease {
        task: KeyedHash,
    },
    WithdrawRelease {
        task: KeyedHash,
    },
    AnswerRelease {
        task: KeyedHash,
        requester: MemberId,
        accept: bool,
    },
    Rekey {
        from: KeyedHash,
        to: KeyedHash,
        epoch: Epoch,
    },
}

/// One event of the book, with its task.
pub type BookEvent = (KeyedHash, ReservationEvent);
/// Events of a book operation, each with its task.
pub type BookEvents = Vec<BookEvent>;

/// All reservations of one team: the authority the server calls.
#[derive(Debug, Clone)]
pub struct ReservationBook {
    policy: Policy,
    epoch: Epoch,
    /// The last key rotation while holds of earlier epochs remain; they stand until
    /// `rotated_at + rekey_grace`.
    rotated_at: Option<UnixMillis>,
    entries: HashMap<KeyedHash, Reservation>,
    /// Open connections per device: a device is online while it has at least one.
    connections: HashMap<Holder, u32>,
}

impl ReservationBook {
    /// An empty book for one team, at key epoch `epoch`.
    pub fn new(policy: Policy, epoch: Epoch) -> Self {
        Self {
            policy,
            epoch,
            rotated_at: None,
            entries: HashMap::new(),
            connections: HashMap::new(),
        }
    }

    /// The key epoch whose hashes may be reserved.
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// The reservation of `task`, if it is held.
    pub fn get(&self, task: &KeyedHash) -> Option<&Reservation> {
        self.entries.get(task)
    }

    /// Every held task, for the snapshot of a resync.
    pub fn iter(&self) -> impl Iterator<Item = (&KeyedHash, &Reservation)> {
        self.entries.iter()
    }

    fn is_online(&self, who: &Holder) -> bool {
        self.connections.get(who).is_some_and(|count| *count > 0)
    }

    fn held_by(&self, member: &MemberId) -> usize {
        self.entries
            .values()
            .filter(|r| r.holder().is_some_and(|h| h.member == *member))
            .count()
    }

    fn has_room(&self, member: &MemberId) -> bool {
        self.held_by(member) < MAX_RESERVATIONS_PER_MEMBER
    }

    /// Applies time to every reservation, ends the rekey grace if it ran out, and drops
    /// entries without a hold. Every operation starts with it, so counts and conflicts never
    /// see a hold that already ran out.
    fn advance_all(&mut self, now: UnixMillis) -> BookEvents {
        let policy = self.policy;
        let mut events = Vec::new();
        for (task, entry) in &mut self.entries {
            events.extend(tag(*task, entry.advance(&policy, now)));
        }
        if let Some(rotated) = self.rotated_at {
            if now >= rotated.plus(policy.rekey_grace_ms) {
                self.rotated_at = None;
                let current = self.epoch;
                for (task, entry) in &mut self.entries {
                    if entry.hold().is_some_and(|hold| hold.epoch < current) {
                        events.extend(tag(*task, entry.expire_now(now)));
                    }
                }
            }
        }
        self.prune();
        events
    }

    /// Applies one intent of `actor` at `now`. The server calls this strictly in arrival order
    /// per team; the first `Reserve` of a free task wins, every later one gets `Conflict`.
    pub fn apply(&mut self, actor: &Holder, intent: Intent, now: UnixMillis) -> Applied<BookEvent> {
        let timing = self.advance_all(now);
        let policy = self.policy;
        let outcome = match intent {
            Intent::Reserve { task, epoch, prev } => self.reserve(actor, task, epoch, &prev, now),
            Intent::Release { task } => self.with(task, |r| r.do_release(actor)),
            Intent::Heartbeat { tasks } => {
                let mut events = Vec::new();
                for task in tasks {
                    if let Some(entry) = self.entries.get_mut(&task) {
                        if let Ok(more) = entry.do_heartbeat(&policy, actor, now) {
                            events.extend(tag(task, more));
                        }
                    }
                }
                Ok(events)
            }
            Intent::RequestRelease { task } => {
                let has_room = self.has_room(&actor.member);
                self.with(task, |r| r.do_request(actor, has_room, now))
            }
            Intent::WithdrawRelease { task } => {
                self.with(task, |r| r.do_withdraw(&policy, actor, now))
            }
            Intent::AnswerRelease {
                task,
                requester,
                accept,
            } => {
                // The task passes to the requesting device, so its presence and its count matter.
                let handover = match self.entries.get(&task).map(Reservation::state) {
                    Some(ReservationState::ReleaseRequested { request, .. }) => Handover {
                        requester_online: self.is_online(&request.requester),
                        requester_has_room: self.has_room(&request.requester.member),
                    },
                    _ => Handover {
                        requester_online: false,
                        requester_has_room: false,
                    },
                };
                self.with(task, |r| {
                    r.do_answer(&policy, actor, &requester, accept, handover, now)
                })
            }
            Intent::Rekey { from, to, epoch } => self.rekey(actor, from, to, epoch, now),
        };
        self.prune();
        Applied { timing, outcome }
    }

    /// `task.force_release` by a member with `role` (from the verified ticket).
    pub fn force_release(
        &mut self,
        role: Role,
        task: KeyedHash,
        now: UnixMillis,
    ) -> Applied<BookEvent> {
        let timing = self.advance_all(now);
        let outcome = self.with(task, |r| r.do_force_release(role));
        self.prune();
        Applied { timing, outcome }
    }

    fn with(
        &mut self,
        task: KeyedHash,
        op: impl FnOnce(&mut Reservation) -> Outcome<ReservationEvent>,
    ) -> Outcome<BookEvent> {
        let entry = self.entries.get_mut(&task).ok_or(Rejection::NotHeld)?;
        op(entry).map(|events| tag(task, events))
    }

    fn reserve(
        &mut self,
        actor: &Holder,
        task: KeyedHash,
        epoch: Epoch,
        prev: &[KeyedHash],
        now: UnixMillis,
    ) -> Outcome<BookEvent> {
        if epoch != self.epoch {
            return Err(Rejection::StaleEpoch);
        }
        if prev.contains(&task) {
            return Err(Rejection::SameTask);
        }
        // The same task under an earlier epoch's hash, held and not rekeyed yet: someone else's
        // is a conflict, the sender's own moves to the new hash.
        let mut own_old = None;
        for old in prev {
            let Some(hold) = self.entries.get(old).and_then(Reservation::hold) else {
                continue;
            };
            if hold.epoch >= self.epoch {
                continue;
            }
            if hold.holder != *actor {
                return Err(Rejection::Conflict {
                    holder: hold.holder.clone(),
                });
            }
            own_old.get_or_insert(*old);
        }
        if let Some(from) = own_old {
            return self.move_hold(actor, from, task, now);
        }
        let policy = self.policy;
        let online = self.is_online(actor);
        let new_hold = self
            .entries
            .get(&task)
            .and_then(Reservation::holder)
            .is_none();
        if new_hold && !self.has_room(&actor.member) {
            return Err(Rejection::Limit);
        }
        let entry = self.entries.entry(task).or_default();
        entry
            .do_reserve(&policy, actor, epoch, online, now)
            .map(|events| tag(task, events))
    }

    fn rekey(
        &mut self,
        actor: &Holder,
        from: KeyedHash,
        to: KeyedHash,
        epoch: Epoch,
        now: UnixMillis,
    ) -> Outcome<BookEvent> {
        if from == to {
            return Err(Rejection::SameTask);
        }
        if epoch != self.epoch {
            return Err(Rejection::StaleEpoch);
        }
        let hold = self
            .entries
            .get(&from)
            .and_then(Reservation::hold)
            .ok_or(Rejection::NotHeld)?;
        if hold.holder != *actor {
            return Err(Rejection::NotHolder);
        }
        if hold.epoch >= self.epoch {
            return Err(Rejection::NotOldEpoch);
        }
        self.move_hold(actor, from, to, now)
    }

    /// Moves `actor`'s hold from an old epoch's hash to the current one, keeping its state.
    fn move_hold(
        &mut self,
        actor: &Holder,
        from: KeyedHash,
        to: KeyedHash,
        now: UnixMillis,
    ) -> Outcome<BookEvent> {
        let policy = self.policy;
        let target = self.entries.get(&to).and_then(Reservation::holder).cloned();
        if let Some(holder) = &target {
            if holder != actor {
                return Err(Rejection::Conflict {
                    holder: holder.clone(),
                });
            }
        }
        let Some(mut moved) = self.entries.remove(&from) else {
            return Err(Rejection::NotHeld);
        };
        let mut events = vec![(
            from,
            ReservationEvent::Released {
                holder: actor.clone(),
                reason: ReleaseReason::Released,
            },
        )];
        if target.is_some() {
            // The sender already holds the new hash: the old one just ends.
            if let Some(entry) = self.entries.get_mut(&to) {
                events.extend(tag(to, entry.touch(&policy, now)));
            }
            return Ok(events);
        }
        if let ReservationState::Held { hold }
        | ReservationState::Expiring { hold, .. }
        | ReservationState::ReleaseRequested { hold, .. } = &mut moved.state
        {
            hold.epoch = self.epoch;
            events.push((to, reserved(hold, &policy)));
        }
        self.entries.insert(to, moved);
        Ok(events)
    }

    /// One connection of a device opened (`online`) or closed. The book counts connections, so
    /// a device with several stays online until its last one closes.
    pub fn presence(&mut self, who: &Holder, online: bool, now: UnixMillis) -> BookEvents {
        let mut events = self.advance_all(now);
        let count = self.connections.entry(who.clone()).or_default();
        let was_online = *count > 0;
        *count = if online {
            count.saturating_add(1)
        } else {
            count.saturating_sub(1)
        };
        let is_online = *count > 0;
        if !is_online {
            self.connections.remove(who);
        }
        if was_online != is_online {
            let policy = self.policy;
            for (task, entry) in &mut self.entries {
                events.extend(tag(*task, entry.presence(&policy, who, is_online, now)));
            }
        }
        self.prune();
        events
    }

    /// Applies time to every reservation (request timeouts, warnings, expiry, the end of the
    /// rekey grace).
    pub fn tick(&mut self, now: UnixMillis) -> BookEvents {
        self.advance_all(now)
    }

    /// When the server should call [`ReservationBook::tick`] next.
    pub fn next_wakeup(&self) -> Option<UnixMillis> {
        let grace = self.rotated_at.filter(|_| {
            self.entries
                .values()
                .any(|entry| entry.hold().is_some_and(|hold| hold.epoch < self.epoch))
        });
        self.entries
            .values()
            .filter_map(|entry| entry.next_wakeup(&self.policy))
            .chain(grace.map(|rotated| rotated.plus(self.policy.rekey_grace_ms)))
            .min()
    }

    /// A member left: their reservations are free, their requests withdrawn.
    pub fn remove_member(&mut self, member: &MemberId, now: UnixMillis) -> BookEvents {
        let mut events = self.advance_all(now);
        self.connections
            .retain(|holder, _| holder.member != *member);
        for (task, entry) in &mut self.entries {
            events.extend(tag(*task, entry.remove_holder(member)));
        }
        self.prune();
        events
    }

    /// A device was removed (its member stays): its reservations are free, its requests end.
    pub fn remove_device(&mut self, device: &Holder, now: UnixMillis) -> BookEvents {
        let mut events = self.advance_all(now);
        self.connections.remove(device);
        for (task, entry) in &mut self.entries {
            events.extend(tag(*task, entry.remove_device(device)));
        }
        self.prune();
        events
    }

    /// A new key epoch (it must be newer): from now on only hashes of this epoch may be
    /// reserved. Holds of earlier epochs stand for the grace period: their holders rekey them
    /// (`task.rekey`, or `task.reserve` with `prev`), and a `task.reserve` naming their old hash
    /// in `prev` conflicts with them. Holds still old when the grace ends expire. A rotation
    /// during the grace starts it again for every old hold.
    pub fn rotate(&mut self, epoch: Epoch, now: UnixMillis) -> Result<BookEvents, Rejection> {
        if epoch <= self.epoch {
            return Err(Rejection::StaleEpoch);
        }
        let events = self.advance_all(now);
        self.epoch = epoch;
        self.rotated_at = Some(now);
        Ok(events)
    }

    /// Free and expired entries carry nothing the next reserve needs.
    fn prune(&mut self) {
        self.entries.retain(|_, entry| entry.holder().is_some());
    }
}

fn tag(task: KeyedHash, events: Vec<ReservationEvent>) -> BookEvents {
    events.into_iter().map(|event| (task, event)).collect()
}

#[cfg(test)]
mod tests;
