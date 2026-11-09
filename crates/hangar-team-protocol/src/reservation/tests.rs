use super::*;
use crate::ids::DeviceId;

const MIN: u64 = MINUTE;

fn who(name: &str) -> Holder {
    Holder {
        member: MemberId::new(name).unwrap(),
        device: DeviceId::new(format!("{name}-pc")).unwrap(),
    }
}

fn member(name: &str) -> MemberId {
    MemberId::new(name).unwrap()
}

fn at(minutes: u64) -> UnixMillis {
    UnixMillis(1_000_000 + minutes * MIN)
}

fn task(n: u8) -> KeyedHash {
    KeyedHash::from_bytes([n; 32])
}

fn kind(event: &ReservationEvent) -> &'static str {
    match event {
        ReservationEvent::Reserved { .. } => "reserved",
        ReservationEvent::Released { .. } => "released",
        ReservationEvent::Expiring { .. } => "expiring",
        ReservationEvent::Expired { .. } => "expired",
        ReservationEvent::ReleaseRequested { .. } => "requested",
        ReservationEvent::ReleaseAnswered { .. } => "answered",
        ReservationEvent::ReleaseRequestEnded { .. } => "request ended",
    }
}

fn kinds(events: &[ReservationEvent]) -> Vec<&'static str> {
    events.iter().map(kind).collect()
}

fn book_kinds(events: &[BookEvent]) -> Vec<&'static str> {
    events.iter().map(|(_, event)| kind(event)).collect()
}

/// The intent's own events; panics when it was refused.
fn ok(applied: Applied<ReservationEvent>) -> Vec<ReservationEvent> {
    applied.outcome.unwrap()
}

fn book_ok(applied: Applied<BookEvent>) -> BookEvents {
    applied.outcome.unwrap()
}

const ROOM: Handover = Handover {
    requester_online: true,
    requester_has_room: true,
};

fn reserve(t: KeyedHash, epoch: u32) -> Intent {
    Intent::Reserve {
        task: t,
        epoch: Epoch(epoch),
        prev: Vec::new(),
    }
}

/// A hold of `a` made offline at minute 0: deadline at minute 30, warning at 25.
fn offline_hold() -> Reservation {
    let mut r = Reservation::default();
    ok(r.reserve(&Policy::default(), &who("a"), Epoch(1), false, at(0)));
    r
}

/// A book in which `a` (offline) holds task 1 since minute 0.
fn book_with_offline_hold() -> ReservationBook {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book
}

#[test]
fn policy_is_checked() {
    assert!(Policy::new(30 * MIN, 5 * MIN, false).is_ok());
    assert_eq!(
        Policy::new(MIN - 1, 1, false),
        Err(PolicyError::InactivityTooShort)
    );
    assert_eq!(
        Policy::new(10 * MIN, 10 * MIN, false),
        Err(PolicyError::WarningOutOfRange)
    );
    assert_eq!(
        Policy::new(10 * MIN, 0, false),
        Err(PolicyError::WarningOutOfRange)
    );
    let default = Policy::default();
    assert_eq!(default.inactivity_ms(), 30 * MIN);
    assert_eq!(default.warn_before_ms(), 5 * MIN);
    assert!(!default.expire_while_online());
    assert_eq!(default.request_timeout_ms(), 10 * MIN);
    assert_eq!(default.request_cooldown_ms(), 10 * MIN);
    assert_eq!(default.rekey_grace_ms(), 30 * MIN);
    assert_eq!(
        default.with_requests(MIN - 1, 0),
        Err(PolicyError::RequestTimeoutTooShort)
    );
    assert_eq!(default.with_rekey_grace(0), Err(PolicyError::GraceTooShort));
    let custom = default
        .with_requests(2 * MIN, 0)
        .unwrap()
        .with_rekey_grace(5 * MIN)
        .unwrap();
    assert_eq!(custom.request_timeout_ms(), 2 * MIN);
    assert_eq!(custom.request_cooldown_ms(), 0);
    assert_eq!(custom.rekey_grace_ms(), 5 * MIN);
}

#[test]
fn free_to_held_with_deadline_only_while_offline() {
    let policy = Policy::default();
    let mut online = Reservation::default();
    let applied = online.reserve(&policy, &who("a"), Epoch(1), true, at(0));
    assert!(applied.timing.is_empty());
    assert_eq!(
        applied.outcome,
        Ok(vec![ReservationEvent::Reserved {
            holder: who("a"),
            epoch: Epoch(1),
            since: at(0),
            expires_at: None,
        }])
    );
    // Online under the default policy: nothing ever runs out.
    assert!(online.advance(&policy, at(10_000)).is_empty());
    assert!(matches!(online.state(), ReservationState::Held { .. }));
    assert_eq!(online.next_wakeup(&policy), None);

    let offline = offline_hold();
    assert_eq!(offline.next_wakeup(&policy), Some(at(25)));
}

#[test]
fn expiry_warns_and_expires_exactly_at_the_boundaries() {
    let policy = Policy::default();
    let mut r = offline_hold();
    assert!(r.advance(&policy, at(25).minus(1)).is_empty());
    assert_eq!(
        r.advance(&policy, at(25)),
        vec![ReservationEvent::Expiring {
            holder: who("a"),
            expires_at: at(30),
        }]
    );
    assert!(matches!(r.state(), ReservationState::Expiring { .. }));
    // Warned once only.
    assert!(r.advance(&policy, at(29)).is_empty());
    assert_eq!(r.next_wakeup(&policy), Some(at(30)));
    assert!(r.advance(&policy, at(30).minus(1)).is_empty());
    assert_eq!(
        r.advance(&policy, at(30)),
        vec![ReservationEvent::Expired { holder: who("a") }]
    );
    assert_eq!(
        r.state(),
        &ReservationState::Expired {
            holder: who("a"),
            at: at(30)
        }
    );
    assert_eq!(r.next_wakeup(&policy), None);
}

#[test]
fn a_late_tick_expires_without_a_warning() {
    let policy = Policy::default();
    let mut r = offline_hold();
    assert_eq!(kinds(&r.advance(&policy, at(45))), vec!["expired"]);
}

#[test]
fn heartbeat_extends_and_withdraws_the_warning() {
    let policy = Policy::default();
    let mut r = offline_hold();
    r.advance(&policy, at(26));
    let events = ok(r.heartbeat(&policy, &who("a"), at(27)));
    assert_eq!(
        events,
        vec![ReservationEvent::Reserved {
            holder: who("a"),
            epoch: Epoch(1),
            since: at(0),
            expires_at: Some(at(57)),
        }]
    );
    assert!(matches!(r.state(), ReservationState::Held { .. }));
    assert_eq!(r.next_wakeup(&policy), Some(at(52)));
}

#[test]
fn heartbeat_after_expiry_is_refused_but_the_expiry_is_reported() {
    let policy = Policy::default();
    let mut r = offline_hold();
    // Exactly at the deadline the hold is gone, also for a heartbeat arriving then.
    let applied = r.heartbeat(&policy, &who("a"), at(30));
    assert_eq!(kinds(&applied.timing), vec!["expired"]);
    assert_eq!(applied.outcome, Err(Rejection::NotHeld));
    assert_eq!(Rejection::NotHeld.code(), ErrorCode::BadRequest);
    // One millisecond earlier it would still have counted.
    let mut r = offline_hold();
    assert!(r
        .heartbeat(&policy, &who("a"), at(30).minus(1))
        .outcome
        .is_ok());
    // Someone else's heartbeat never extends.
    assert_eq!(
        r.heartbeat(&policy, &who("b"), at(31)).outcome,
        Err(Rejection::NotHolder)
    );
}

#[test]
fn reserve_by_the_holder_counts_as_activity_and_others_conflict() {
    let policy = Policy::default();
    let mut r = offline_hold();
    assert_eq!(
        kinds(&ok(r.reserve(&policy, &who("a"), Epoch(1), false, at(10)))),
        vec!["reserved"]
    );
    assert_eq!(
        r.reserve(&policy, &who("b"), Epoch(1), true, at(11))
            .outcome,
        Err(Rejection::Conflict { holder: who("a") })
    );
    // After expiry the other member wins; the expiry comes first, as timing.
    let applied = r.reserve(&policy, &who("b"), Epoch(1), true, at(40));
    assert_eq!(kinds(&applied.timing), vec!["expired"]);
    assert_eq!(kinds(&applied.outcome.unwrap()), vec!["reserved"]);
    assert_eq!(r.holder(), Some(&who("b")));
    // Another device conflicts too, even of the same member.
    let mut other_device = who("b");
    other_device.device = DeviceId::new("b-laptop").unwrap();
    assert!(matches!(
        r.reserve(&policy, &other_device, Epoch(1), true, at(41))
            .outcome,
        Err(Rejection::Conflict { .. })
    ));
}

#[test]
fn release_by_holder_only() {
    let policy = Policy::default();
    let mut r = offline_hold();
    assert_eq!(
        r.release(&policy, &who("b"), at(1)).outcome,
        Err(Rejection::NotHolder)
    );
    assert_eq!(
        ok(r.release(&policy, &who("a"), at(1))),
        vec![ReservationEvent::Released {
            holder: who("a"),
            reason: ReleaseReason::Released
        }]
    );
    assert_eq!(r.state(), &ReservationState::Free);
    // Nothing to ask for or to answer on a free task.
    assert_eq!(
        r.request_release(&policy, &who("b"), true, at(2)).outcome,
        Err(Rejection::NotHeld)
    );
    assert_eq!(
        r.answer_release(&policy, &who("a"), &member("b"), true, ROOM, at(2))
            .outcome,
        Err(Rejection::NotHeld)
    );
    assert_eq!(
        r.release(&policy, &who("a"), at(2)).outcome,
        Err(Rejection::NotHeld)
    );
}

#[test]
fn release_request_accepted_hands_over() {
    let policy = Policy::default();
    let mut r = offline_hold();
    assert_eq!(
        r.request_release(&policy, &who("a"), true, at(1)).outcome,
        Err(Rejection::OwnReservation)
    );
    assert!(matches!(r.state(), ReservationState::Held { .. }));
    assert_eq!(
        kinds(&ok(r.request_release(&policy, &who("b"), true, at(2)))),
        vec!["requested"]
    );
    assert_eq!(
        r.request_release(&policy, &who("c"), true, at(3)).outcome,
        Err(Rejection::RequestPending)
    );
    assert_eq!(Rejection::RequestPending.code(), ErrorCode::Conflict);
    assert_eq!(
        r.answer_release(&policy, &who("b"), &member("b"), true, ROOM, at(4))
            .outcome,
        Err(Rejection::NotHolder)
    );
    assert_eq!(
        r.answer_release(&policy, &who("a"), &member("c"), true, ROOM, at(4))
            .outcome,
        Err(Rejection::NoRequest)
    );
    let events = ok(r.answer_release(&policy, &who("a"), &member("b"), true, ROOM, at(5)));
    assert_eq!(kinds(&events), vec!["answered", "released", "reserved"]);
    assert_eq!(r.holder(), Some(&who("b")));
    assert!(matches!(
        &events[1],
        ReservationEvent::Released {
            reason: ReleaseReason::HandedOver,
            ..
        }
    ));
}

#[test]
fn release_request_declined_keeps_the_hold_and_its_warning() {
    let policy = Policy::default();
    let mut r = offline_hold();
    r.advance(&policy, at(26));
    ok(r.request_release(&policy, &who("b"), true, at(27)));
    assert!(matches!(
        r.state(),
        ReservationState::ReleaseRequested {
            warned_at: Some(_),
            ..
        }
    ));
    let events = ok(r.answer_release(&policy, &who("a"), &member("b"), false, ROOM, at(28)));
    assert_eq!(
        events,
        vec![ReservationEvent::ReleaseAnswered {
            holder: who("a"),
            requester: who("b"),
            accepted: false
        }]
    );
    assert!(matches!(r.state(), ReservationState::Expiring { .. }));
    assert_eq!(
        r.answer_release(&policy, &who("a"), &member("b"), false, ROOM, at(28))
            .outcome,
        Err(Rejection::NoRequest)
    );
}

#[test]
fn a_pending_request_does_not_stop_expiry() {
    let policy = Policy::default().with_requests(60 * MIN, 0).unwrap();
    let mut r = offline_hold();
    ok(r.request_release(&policy, &who("b"), true, at(1)));
    assert_eq!(kinds(&r.advance(&policy, at(25))), vec!["expiring"]);
    assert!(r.advance(&policy, at(26)).is_empty());
    assert_eq!(kinds(&r.advance(&policy, at(30))), vec!["expired"]);
    // The requester can simply reserve now.
    assert!(r
        .reserve(&policy, &who("b"), Epoch(1), true, at(31))
        .outcome
        .is_ok());
    // An answer to a request that expired with its hold.
    assert_eq!(
        r.answer_release(&policy, &who("a"), &member("b"), true, ROOM, at(32))
            .outcome,
        Err(Rejection::NotHolder)
    );
}

#[test]
fn heartbeat_during_a_request_keeps_the_request() {
    let policy = Policy::default().with_requests(60 * MIN, 0).unwrap();
    let mut r = offline_hold();
    ok(r.request_release(&policy, &who("b"), true, at(1)));
    r.advance(&policy, at(25));
    ok(r.heartbeat(&policy, &who("a"), at(26)));
    assert!(matches!(
        r.state(),
        ReservationState::ReleaseRequested {
            warned_at: None,
            ..
        }
    ));
    // The new deadline gets its own warning.
    assert_eq!(kinds(&r.advance(&policy, at(51))), vec!["expiring"]);
}

#[test]
fn going_offline_starts_the_clock_with_a_warning_window() {
    let policy = Policy::default();
    let mut r = Reservation::default();
    ok(r.reserve(&policy, &who("a"), Epoch(1), true, at(0)));
    // Two hours idle online, then offline: not expired at once, warned first.
    let events = r.presence(&policy, &who("a"), false, at(120));
    assert_eq!(
        events,
        vec![
            ReservationEvent::Reserved {
                holder: who("a"),
                epoch: Epoch(1),
                since: at(0),
                expires_at: Some(at(125)),
            },
            ReservationEvent::Expiring {
                holder: who("a"),
                expires_at: at(125),
            }
        ]
    );
    // Back online before the deadline: the hold stands again without a deadline.
    let events = r.presence(&policy, &who("a"), true, at(124));
    assert_eq!(kinds(&events), vec!["reserved"]);
    assert!(matches!(r.state(), ReservationState::Held { .. }));
    // Presence of someone else changes nothing.
    assert!(r.presence(&policy, &who("b"), false, at(130)).is_empty());
    // Repeating the same presence changes nothing.
    assert!(r.presence(&policy, &who("a"), true, at(131)).is_empty());
}

#[test]
fn expire_while_online_runs_the_clock_always() {
    let policy = Policy::new(30 * MIN, 5 * MIN, true).unwrap();
    let mut r = Reservation::default();
    ok(r.reserve(&policy, &who("a"), Epoch(1), true, at(0)));
    assert_eq!(kinds(&r.advance(&policy, at(25))), vec!["expiring"]);
    assert_eq!(kinds(&r.advance(&policy, at(30))), vec!["expired"]);
}

// --- R1: time events survive a refused intent, once ---------------------------------------

#[test]
fn a_refused_reserve_still_warns_the_holder() {
    let mut book = book_with_offline_hold();
    let applied = book.apply(&who("b"), reserve(task(1), 1), at(26));
    assert_eq!(
        applied.timing,
        vec![(
            task(1),
            ReservationEvent::Expiring {
                holder: who("a"),
                expires_at: at(30)
            }
        )]
    );
    assert_eq!(
        applied.outcome,
        Err(Rejection::Conflict { holder: who("a") })
    );
    // Not again from a later tick.
    assert!(book.tick(at(27)).is_empty());
    assert_eq!(book_kinds(&book.tick(at(30))), vec!["expired"]);
    assert!(book.tick(at(31)).is_empty());
}

#[test]
fn intents_after_the_deadline_report_the_expiry() {
    let intents = [
        (who("b"), Intent::RequestRelease { task: task(1) }),
        (who("a"), Intent::Release { task: task(1) }),
        (
            who("a"),
            Intent::Rekey {
                from: task(1),
                to: task(2),
                epoch: Epoch(1),
            },
        ),
        (
            who("a"),
            Intent::AnswerRelease {
                task: task(1),
                requester: member("b"),
                accept: true,
            },
        ),
        (who("a"), Intent::WithdrawRelease { task: task(1) }),
    ];
    for (actor, intent) in intents {
        let mut book = book_with_offline_hold();
        let applied = book.apply(&actor, intent.clone(), at(31));
        assert_eq!(book_kinds(&applied.timing), vec!["expired"], "{intent:?}");
        assert!(applied.outcome.is_err(), "{intent:?}");
        assert!(book.tick(at(32)).is_empty(), "{intent:?}");
        assert!(book.get(&task(1)).is_none());
    }
    // A rekey after a rotation at minute 31: the expiry, then the refusal.
    let mut book = book_with_offline_hold();
    let rotated = book.rotate(Epoch(2), at(31)).unwrap();
    assert_eq!(book_kinds(&rotated), vec!["expired"]);
    let applied = book.apply(
        &who("a"),
        Intent::Rekey {
            from: task(1),
            to: task(2),
            epoch: Epoch(2),
        },
        at(32),
    );
    assert!(applied.timing.is_empty());
    assert_eq!(applied.outcome, Err(Rejection::NotHeld));
}

#[test]
fn applied_events_come_in_order() {
    let mut book = book_with_offline_hold();
    let (events, rejection) = book
        .apply(&who("b"), reserve(task(1), 1), at(40))
        .into_events();
    assert_eq!(book_kinds(&events), vec!["expired", "reserved"]);
    assert_eq!(rejection, None);
    let (events, rejection) = book
        .apply(&who("c"), reserve(task(1), 1), at(41))
        .into_events();
    assert!(events.is_empty());
    assert_eq!(rejection, Some(Rejection::Conflict { holder: who("b") }));
}

#[test]
fn a_rekey_onto_an_expired_hold_succeeds() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), reserve(task(2), 1), at(0)));
    book.presence(&who("a"), true, at(0));
    book.rotate(Epoch(2), at(5)).unwrap();
    // b's hold on task 2 expired at minute 30, before any tick (the grace runs to 35).
    let applied = book.apply(
        &who("a"),
        Intent::Rekey {
            from: task(1),
            to: task(2),
            epoch: Epoch(2),
        },
        at(31),
    );
    assert_eq!(
        applied.timing,
        vec![(task(2), ReservationEvent::Expired { holder: who("b") })]
    );
    assert_eq!(
        book_kinds(&applied.outcome.unwrap()),
        vec!["released", "reserved"]
    );
}

// --- R2 and N3: request timeout, withdrawal, cooldown -------------------------------------

#[test]
fn an_unanswered_request_times_out_and_the_requester_cools_down() {
    let policy = Policy::default();
    let mut book = ReservationBook::new(policy, Epoch(1));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(1)));
    assert_eq!(book.next_wakeup(), Some(at(11)));
    assert!(book.tick(at(11).minus(1)).is_empty());
    assert_eq!(
        book.tick(at(11)),
        vec![(
            task(1),
            ReservationEvent::ReleaseRequestEnded {
                holder: who("a"),
                requester: who("b"),
                reason: RequestEndReason::TimedOut
            }
        )]
    );
    assert!(matches!(
        book.get(&task(1)).unwrap().state(),
        ReservationState::Held { .. }
    ));
    assert_eq!(book.next_wakeup(), None);
    // Asking again right away is refused until 10 minutes after the timeout.
    let refused = book
        .apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(12))
        .outcome
        .unwrap_err();
    assert_eq!(refused, Rejection::Cooldown { until: at(21) });
    assert_eq!(refused.code(), ErrorCode::RateLimited);
    assert_eq!(refused.retry_after_ms(at(12)), Some(9 * MIN));
    // Another member is not affected; after the cooldown b may ask again.
    let mut other = book.clone();
    assert!(other
        .apply(&who("c"), Intent::RequestRelease { task: task(1) }, at(12))
        .outcome
        .is_ok());
    assert!(book
        .apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(21))
        .outcome
        .is_ok());
}

#[test]
fn a_late_tick_times_out_the_request_before_the_hold_expires() {
    let policy = Policy::default();
    let mut r = offline_hold();
    ok(r.request_release(&policy, &who("b"), true, at(1)));
    // Timeout at 11 comes before the deadline at 30.
    assert_eq!(
        kinds(&r.advance(&policy, at(40))),
        vec!["request ended", "expired"]
    );
    // A request made late in the hold: the expiry takes the request with it.
    let mut r = offline_hold();
    ok(r.request_release(&policy, &who("b"), true, at(25)));
    assert_eq!(kinds(&r.advance(&policy, at(40))), vec!["expired"]);
}

#[test]
fn a_decline_starts_a_cooldown() {
    let policy = Policy::default();
    let mut r = offline_hold();
    ok(r.request_release(&policy, &who("b"), true, at(1)));
    ok(r.answer_release(&policy, &who("a"), &member("b"), false, ROOM, at(2)));
    // Also from another device of the same member.
    let mut laptop = who("b");
    laptop.device = DeviceId::new("b-laptop").unwrap();
    assert_eq!(
        r.request_release(&policy, &laptop, true, at(3)).outcome,
        Err(Rejection::Cooldown { until: at(12) })
    );
    assert!(r
        .request_release(&policy, &who("b"), true, at(12))
        .outcome
        .is_ok());
}

#[test]
fn the_requester_can_withdraw() {
    let policy = Policy::default();
    let mut book = ReservationBook::new(policy, Epoch(1));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(1)));
    // Only the requester may take it back.
    for actor in [who("a"), who("c")] {
        assert_eq!(
            book.apply(&actor, Intent::WithdrawRelease { task: task(1) }, at(2))
                .outcome,
            Err(Rejection::NoRequest)
        );
    }
    let events = book_ok(book.apply(&who("b"), Intent::WithdrawRelease { task: task(1) }, at(2)));
    assert_eq!(
        events,
        vec![(
            task(1),
            ReservationEvent::ReleaseRequestEnded {
                holder: who("a"),
                requester: who("b"),
                reason: RequestEndReason::Withdrawn
            }
        )]
    );
    assert_eq!(
        book.apply(&who("b"), Intent::WithdrawRelease { task: task(1) }, at(3))
            .outcome,
        Err(Rejection::NoRequest)
    );
    // Withdrawing and asking again cannot be used to flood the holder.
    assert!(matches!(
        book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(3))
            .outcome,
        Err(Rejection::Cooldown { .. })
    ));
    // The cooldown ends with the hold: a new holder can be asked at once.
    book_ok(book.apply(&who("a"), Intent::Release { task: task(1) }, at(4)));
    book_ok(book.apply(&who("c"), reserve(task(1), 1), at(5)));
    assert!(book
        .apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(6))
        .outcome
        .is_ok());
}

// --- R3: removing a device -----------------------------------------------------------------

#[test]
fn removing_a_device_frees_its_holds_and_ends_its_requests() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    let mut laptop = who("a");
    laptop.device = DeviceId::new("a-laptop").unwrap();
    book.presence(&laptop, true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&laptop, reserve(task(2), 1), at(0)));
    book_ok(book.apply(&who("b"), reserve(task(3), 1), at(0)));
    book_ok(book.apply(&laptop, Intent::RequestRelease { task: task(3) }, at(1)));
    let mut events = book.remove_device(&laptop, at(2));
    events.sort_by_key(|(task, _)| *task);
    assert_eq!(
        events,
        vec![
            (
                task(2),
                ReservationEvent::Released {
                    holder: laptop.clone(),
                    reason: ReleaseReason::MemberRemoved
                }
            ),
            (
                task(3),
                ReservationEvent::ReleaseRequestEnded {
                    holder: who("b"),
                    requester: laptop.clone(),
                    reason: RequestEndReason::Withdrawn
                }
            ),
        ]
    );
    // The member's other device keeps its hold; the removed one is no longer online.
    assert!(book.get(&task(1)).is_some());
    assert!(book.get(&task(2)).is_none());
    assert!(!book.is_online(&laptop));
    // The member may ask again from the remaining device (no cooldown).
    assert!(book
        .apply(&who("a"), Intent::RequestRelease { task: task(3) }, at(3))
        .outcome
        .is_ok());
}

// --- R4: limits on a handover, rekey onto itself -------------------------------------------

#[test]
fn a_handover_respects_the_requesters_limit() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    for n in 0..MAX_RESERVATIONS_PER_MEMBER - 1 {
        book_ok(book.apply(&who("b"), reserve(task(u8::try_from(n).unwrap()), 1), at(0)));
    }
    book_ok(book.apply(&who("a"), reserve(task(200), 1), at(0)));
    book_ok(book.apply(&who("a"), reserve(task(201), 1), at(0)));
    // b has room for one more: it may ask for both.
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(200) }, at(1)));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(201) }, at(1)));
    let accept = |t| Intent::AnswerRelease {
        task: t,
        requester: member("b"),
        accept: true,
    };
    book_ok(book.apply(&who("a"), accept(task(200)), at(2)));
    // Now b is full: the second accept is refused and the request stays open.
    let refused = book.apply(&who("a"), accept(task(201)), at(3)).outcome;
    assert_eq!(refused, Err(Rejection::Limit));
    assert!(matches!(
        book.get(&task(201)).unwrap().state(),
        ReservationState::ReleaseRequested { .. }
    ));
    // A full member cannot ask at all.
    book_ok(book.apply(&who("a"), reserve(task(202), 1), at(3)));
    assert_eq!(
        book.apply(&who("b"), Intent::RequestRelease { task: task(202) }, at(4))
            .outcome,
        Err(Rejection::Limit)
    );
    // Declining still works.
    let decline = Intent::AnswerRelease {
        task: task(201),
        requester: member("b"),
        accept: false,
    };
    assert!(book.apply(&who("a"), decline, at(5)).outcome.is_ok());
}

#[test]
fn a_rekey_onto_the_same_hash_is_refused() {
    let mut book = book_with_offline_hold();
    book.rotate(Epoch(2), at(1)).unwrap();
    let applied = book.apply(
        &who("a"),
        Intent::Rekey {
            from: task(1),
            to: task(1),
            epoch: Epoch(2),
        },
        at(2),
    );
    assert_eq!(applied.outcome, Err(Rejection::SameTask));
    assert_eq!(Rejection::SameTask.code(), ErrorCode::BadRequest);
}

// --- Book basics ---------------------------------------------------------------------------

#[test]
fn two_reserves_the_first_wins() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book.presence(&who("b"), true, at(0));
    // Both arrive within the same millisecond; the server applies them in arrival order.
    let first = book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    assert_eq!(book_kinds(&first), vec!["reserved"]);
    let rejection = book
        .apply(&who("b"), reserve(task(1), 1), at(0))
        .outcome
        .unwrap_err();
    assert_eq!(rejection, Rejection::Conflict { holder: who("a") });
    assert_eq!(rejection.code(), ErrorCode::Conflict);
    assert_eq!(rejection.holder(), Some(&who("a")));
    assert_eq!(book.get(&task(1)).unwrap().holder(), Some(&who("a")));
    // A retry of the winner is harmless.
    assert!(book
        .apply(&who("a"), reserve(task(1), 1), at(0))
        .outcome
        .is_ok());
}

#[test]
fn book_ticks_expires_and_prunes() {
    // `a` is not connected: its hold runs at once.
    let mut book = book_with_offline_hold();
    assert_eq!(book.next_wakeup(), Some(at(25)));
    assert_eq!(book_kinds(&book.tick(at(25))), vec!["expiring"]);
    assert_eq!(book.next_wakeup(), Some(at(30)));
    assert_eq!(book_kinds(&book.tick(at(30))), vec!["expired"]);
    assert!(book.get(&task(1)).is_none());
    assert_eq!(book.next_wakeup(), None);
    // A heartbeat for a task that is gone is skipped, not an error.
    let applied = book.apply(
        &who("a"),
        Intent::Heartbeat {
            tasks: vec![task(1)],
        },
        at(31),
    );
    assert!(applied.timing.is_empty());
    assert_eq!(applied.outcome, Ok(Vec::new()));
}

#[test]
fn book_checks_epochs_and_limits() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(2));
    let stale = book.apply(&who("a"), reserve(task(1), 1), at(0)).outcome;
    assert_eq!(stale, Err(Rejection::StaleEpoch));
    assert_eq!(Rejection::StaleEpoch.code(), ErrorCode::StaleEpoch);
    for n in 0..MAX_RESERVATIONS_PER_MEMBER {
        book_ok(book.apply(&who("a"), reserve(task(u8::try_from(n).unwrap()), 2), at(0)));
    }
    assert_eq!(
        book.apply(&who("a"), reserve(task(200), 2), at(0)).outcome,
        Err(Rejection::Limit)
    );
    // Refreshing a held one is still fine.
    assert!(book
        .apply(&who("a"), reserve(task(0), 2), at(1))
        .outcome
        .is_ok());
    assert_eq!(
        book.apply(&who("a"), Intent::Release { task: task(250) }, at(1))
            .outcome,
        Err(Rejection::NotHeld)
    );
}

#[test]
fn rekey_moves_the_hold_to_the_new_epoch() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), reserve(task(3), 1), at(0)));
    book_ok(book.apply(&who("c"), reserve(task(5), 1), at(0)));
    assert_eq!(book.rotate(Epoch(1), at(1)), Err(Rejection::StaleEpoch));
    book.rotate(Epoch(2), at(1)).unwrap();
    book_ok(book.apply(&who("c"), reserve(task(6), 2), at(1)));
    let rekey = |from, to, epoch| Intent::Rekey { from, to, epoch };
    let outcome = |book: &mut ReservationBook, actor: &str, intent| {
        book.apply(&who(actor), intent, at(1)).outcome
    };
    assert_eq!(
        outcome(&mut book, "a", rekey(task(1), task(2), Epoch(1))),
        Err(Rejection::StaleEpoch)
    );
    assert_eq!(
        outcome(&mut book, "b", rekey(task(1), task(2), Epoch(2))),
        Err(Rejection::NotHolder)
    );
    assert!(matches!(
        outcome(&mut book, "a", rekey(task(1), task(3), Epoch(2))),
        Err(Rejection::Conflict { .. })
    ));
    // A hold already under the current epoch is not moved around.
    assert_eq!(
        outcome(&mut book, "c", rekey(task(6), task(7), Epoch(2))),
        Err(Rejection::NotOldEpoch)
    );
    let events = book_ok(book.apply(&who("a"), rekey(task(1), task(2), Epoch(2)), at(1)));
    assert_eq!(book_kinds(&events), vec!["released", "reserved"]);
    assert_eq!(events[1].0, task(2));
    assert!(book.get(&task(1)).is_none());
    assert!(matches!(
        book.get(&task(2)).unwrap().state(),
        ReservationState::Held { hold } if hold.epoch == Epoch(2)
    ));
    // c already holds the new hash of its old task: the old one just ends.
    book_ok(book.apply(&who("c"), reserve(task(8), 2), at(1)));
    let events = book_ok(book.apply(&who("c"), rekey(task(5), task(8), Epoch(2)), at(2)));
    assert_eq!(book_kinds(&events), vec!["released", "reserved"]);
    assert!(book.get(&task(5)).is_none());
}

// --- M3: the rotation window -----------------------------------------------------------------

#[test]
fn an_offline_holder_keeps_the_task_through_a_rotation() {
    // a holds task 1 (epoch 1 hash) and is offline; after the rotation the same task's hash
    // under epoch 2 is task 11.
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("b"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book.rotate(Epoch(2), at(1)).unwrap();
    let with_prev = |t, prev: Vec<KeyedHash>| Intent::Reserve {
        task: t,
        epoch: Epoch(2),
        prev,
    };
    // b names the old hash it can compute: conflict with a.
    assert_eq!(
        book.apply(&who("b"), with_prev(task(11), vec![task(1)]), at(2))
            .outcome,
        Err(Rejection::Conflict { holder: who("a") })
    );
    assert!(book.get(&task(11)).is_none());
    // `prev` must not name the new hash itself.
    assert_eq!(
        book.apply(&who("b"), with_prev(task(11), vec![task(11)]), at(2))
            .outcome,
        Err(Rejection::SameTask)
    );
    // a comes back and reserves with its old hash: that moves the hold.
    let events = book_ok(book.apply(&who("a"), with_prev(task(11), vec![task(1)]), at(3)));
    assert_eq!(book_kinds(&events), vec!["released", "reserved"]);
    assert!(matches!(
        book.get(&task(11)).unwrap().state(),
        ReservationState::Held { hold } if hold.epoch == Epoch(2) && hold.since == at(0)
    ));
    // `prev` only counts for holds of earlier epochs: a current hold under that hash is
    // another task and no conflict.
    book_ok(book.apply(&who("a"), reserve(task(20), 2), at(4)));
    assert!(book
        .apply(&who("b"), with_prev(task(21), vec![task(20)]), at(4))
        .outcome
        .is_ok());
}

#[test]
fn old_holds_expire_when_the_grace_ends() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book.presence(&who("b"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), reserve(task(2), 1), at(0)));
    book.rotate(Epoch(2), at(10)).unwrap();
    // b rekeys, a does not (an online hold has no deadline of its own).
    book_ok(book.apply(
        &who("b"),
        Intent::Rekey {
            from: task(2),
            to: task(12),
            epoch: Epoch(2),
        },
        at(11),
    ));
    assert_eq!(book.next_wakeup(), Some(at(40)));
    assert!(book.tick(at(40).minus(1)).is_empty());
    assert_eq!(
        book.tick(at(40)),
        vec![(task(1), ReservationEvent::Expired { holder: who("a") })]
    );
    assert!(book.get(&task(12)).is_some());
    assert_eq!(book.next_wakeup(), None);
}

// --- N4: force release ---------------------------------------------------------------------

#[test]
fn owners_and_admins_can_end_any_hold() {
    let mut book = book_with_offline_hold();
    let refused = book.force_release(Role::Member, task(1), at(1)).outcome;
    assert_eq!(refused, Err(Rejection::NotAllowed));
    assert_eq!(Rejection::NotAllowed.code(), ErrorCode::Forbidden);
    for role in [Role::Admin, Role::Owner] {
        let mut book = book_with_offline_hold();
        let events = book_ok(book.force_release(role, task(1), at(1)));
        assert_eq!(
            events,
            vec![(
                task(1),
                ReservationEvent::Released {
                    holder: who("a"),
                    reason: ReleaseReason::ForceReleased
                }
            )]
        );
        assert!(book.get(&task(1)).is_none());
    }
    assert_eq!(
        book.force_release(Role::Owner, task(9), at(1)).outcome,
        Err(Rejection::NotHeld)
    );
    // Time is applied first here too.
    let applied = book.force_release(Role::Owner, task(1), at(31));
    assert_eq!(book_kinds(&applied.timing), vec!["expired"]);
    assert_eq!(applied.outcome, Err(Rejection::NotHeld));
}

// --- N5: presence per connection -----------------------------------------------------------

#[test]
fn presence_counts_connections() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    // One of two connections closes: the device is still online, nothing changes.
    assert!(book.presence(&who("a"), false, at(1)).is_empty());
    assert!(book.is_online(&who("a")));
    // The last one closes: the clock starts.
    let events = book.presence(&who("a"), false, at(2));
    assert!(matches!(
        events.as_slice(),
        [(_, ReservationEvent::Reserved { expires_at: Some(deadline), .. })] if *deadline == at(30)
    ));
    // A close without an open connection changes nothing.
    assert!(book.presence(&who("a"), false, at(3)).is_empty());
    let events = book.presence(&who("a"), true, at(4));
    assert_eq!(book_kinds(&events), vec!["reserved"]);
}

// --- Members -------------------------------------------------------------------------------

#[test]
fn removing_a_member_frees_their_tasks() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    for (n, name) in [(1, "a"), (2, "b")] {
        book_ok(book.apply(&who(name), reserve(task(n), 1), at(0)));
    }
    let events = book.remove_member(&member("a"), at(1));
    assert_eq!(
        events,
        vec![(
            task(1),
            ReservationEvent::Released {
                holder: who("a"),
                reason: ReleaseReason::MemberRemoved
            }
        )]
    );
    assert!(book.get(&task(1)).is_none());
    assert!(book.get(&task(2)).is_some());
}

#[test]
fn a_removed_requester_never_gets_the_task() {
    let mut book = book_with_offline_hold();
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(1)));
    assert!(book.remove_member(&member("b"), at(1)).is_empty());
    assert!(matches!(
        book.get(&task(1)).unwrap().state(),
        ReservationState::Held { hold } if hold.holder == who("a")
    ));
    assert_eq!(
        book.apply(
            &who("a"),
            Intent::AnswerRelease {
                task: task(1),
                requester: member("b"),
                accept: true,
            },
            at(2),
        )
        .outcome,
        Err(Rejection::NoRequest)
    );
    // Someone else may ask now.
    assert!(book
        .apply(&who("c"), Intent::RequestRelease { task: task(1) }, at(3))
        .outcome
        .is_ok());
}

#[test]
fn book_release_flow_with_presence() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book.presence(&who("b"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(1)));
    let events = book_ok(book.apply(
        &who("a"),
        Intent::AnswerRelease {
            task: task(1),
            requester: member("b"),
            accept: true,
        },
        at(2),
    ));
    // `b` is online: the new hold has no deadline.
    assert!(matches!(
        events.last(),
        Some((_, ReservationEvent::Reserved { expires_at: None, holder, .. })) if *holder == who("b")
    ));
    // `b` disconnects: its hold runs from its last activity (the handover at minute 2).
    let events = book.presence(&who("b"), false, at(3));
    assert!(matches!(
        events.as_slice(),
        [(_, ReservationEvent::Reserved { expires_at: Some(deadline), .. })] if *deadline == at(32)
    ));
}

// --- Persistence across a relay restart ----------------------------------------------------

/// Online `a` holds 1 and 3 (3 with an open request of `b` since minute 20 and a cooldown of
/// `d` until 28); offline `c` holds 2 since minute 0 and was warned at 25 (deadline 30). All
/// three are of epoch 1; the book rotated to 2 at minute 21.
fn busy_book() -> ReservationBook {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book.presence(&who("b"), true, at(0));
    book.presence(&who("d"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("c"), reserve(task(2), 1), at(0)));
    book_ok(book.apply(&who("a"), reserve(task(3), 1), at(0)));
    book_ok(book.apply(&who("d"), Intent::RequestRelease { task: task(3) }, at(17)));
    book_ok(book.apply(
        &who("a"),
        Intent::AnswerRelease {
            task: task(3),
            requester: member("d"),
            accept: false,
        },
        at(18),
    ));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(3) }, at(20)));
    book.rotate(Epoch(2), at(21)).unwrap();
    assert_eq!(book_kinds(&book.tick(at(25))), vec!["expiring"]);
    book
}

#[test]
fn a_snapshot_round_trips_through_json() {
    let book = busy_book();
    let snapshot = book.export();
    assert_eq!(snapshot.version, BOOK_SNAPSHOT_VERSION);
    assert_eq!(snapshot.epoch, Epoch(2));
    assert_eq!(snapshot.rotated_at, Some(at(21)));
    let tasks: Vec<_> = snapshot.entries.iter().map(|e| e.task).collect();
    assert_eq!(tasks, vec![task(1), task(2), task(3)]);
    let expiring = &snapshot.entries[1];
    assert_eq!(expiring.warned_at, Some(at(25)));
    assert_eq!(expiring.offline_since, Some(at(0)));
    let requested = &snapshot.entries[2];
    assert_eq!(
        requested.request,
        Some(ReleaseRequest {
            requester: who("b"),
            at: at(20)
        })
    );
    assert_eq!(
        requested.cooldowns,
        vec![CooldownSnapshot {
            member: member("d"),
            until: at(28)
        }]
    );
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(
        json.starts_with(r#"{"version":1,"epoch":2,"rotatedAt":"#),
        "{json}"
    );
    assert!(json.contains(r#""lastActivity":"#), "{json}");
    assert_eq!(BookSnapshot::from_json(&json).unwrap(), snapshot);
    // Restored and exported again: everyone is offline from now, and the holds of the online
    // `a`, idle since minute 0, now run out at 31 and are warned at once. Nothing else changed.
    let restored = ReservationBook::restore(snapshot.clone(), Policy::default(), at(26)).unwrap();
    assert_eq!(
        book_kinds(&restored.events),
        vec!["reserved", "reserved", "reserved", "expiring", "expiring"]
    );
    let mut again = restored.book.export();
    for entry in &again.entries {
        assert_eq!(entry.offline_since, Some(at(26)));
    }
    assert_eq!(again.entries[0].warned_at, Some(at(26)));
    assert_eq!(again.entries[2].warned_at, Some(at(26)));
    for (a, b) in again.entries.iter_mut().zip(&snapshot.entries) {
        a.offline_since = b.offline_since;
        a.warned_at = b.warned_at;
    }
    assert_eq!(again, snapshot);
}

#[test]
fn after_a_restore_an_online_hold_runs_with_a_warning_first() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(10)).unwrap();
    // The deadline starts to run: max(0 + 30, 10 + 5).
    assert_eq!(
        restored.events,
        vec![(
            task(1),
            ReservationEvent::Reserved {
                holder: who("a"),
                epoch: Epoch(1),
                since: at(0),
                expires_at: Some(at(30)),
            }
        )]
    );
    let mut book = restored.book;
    assert_eq!(book.next_wakeup(), Some(at(25)));
    assert!(book.tick(at(25).minus(1)).is_empty());
    assert_eq!(
        book.tick(at(25)),
        vec![(
            task(1),
            ReservationEvent::Expiring {
                holder: who("a"),
                expires_at: at(30)
            }
        )]
    );
    assert_eq!(
        book.tick(at(30)),
        vec![(task(1), ReservationEvent::Expired { holder: who("a") })]
    );
}

#[test]
fn the_downtime_never_expires_a_hold_without_a_warning() {
    // Offline since minute 0, deadline 30; the relay was down until minute 100.
    let book = book_with_offline_hold();
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(100)).unwrap();
    assert_eq!(book_kinds(&restored.events), vec!["reserved", "expiring"]);
    assert_eq!(
        restored.events[1].1,
        ReservationEvent::Expiring {
            holder: who("a"),
            expires_at: at(105)
        }
    );
    let mut book = restored.book;
    assert!(book.tick(at(105).minus(1)).is_empty());
    assert_eq!(
        book.tick(at(105)),
        vec![(task(1), ReservationEvent::Expired { holder: who("a") })]
    );
}

#[test]
fn a_short_restart_keeps_an_offline_deadline() {
    let book = book_with_offline_hold();
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(5)).unwrap();
    // max(0 + 30, 5 + 5) is the old deadline: nothing to report.
    assert!(restored.events.is_empty());
    let mut book = restored.book;
    assert_eq!(book_kinds(&book.tick(at(25))), vec!["expiring"]);
    assert_eq!(book_kinds(&book.tick(at(30))), vec!["expired"]);
}

#[test]
fn a_reconnect_after_a_restore_stops_the_clock() {
    let book = busy_book();
    let mut book = ReservationBook::restore(book.export(), Policy::default(), at(31))
        .unwrap()
        .book;
    let events = book.presence(&who("c"), true, at(32));
    // The warned hold of 2 (deadline moved from 30 to 36 by the restart) has no deadline any
    // more.
    assert_eq!(
        events,
        vec![(
            task(2),
            ReservationEvent::Reserved {
                holder: who("c"),
                epoch: Epoch(1),
                since: at(0),
                expires_at: None,
            }
        )]
    );
    assert!(matches!(
        book.get(&task(2)).map(Reservation::state),
        Some(ReservationState::Held { .. })
    ));
}

#[test]
fn requests_and_cooldowns_survive_a_restart() {
    let book = busy_book();
    let mut book = ReservationBook::restore(book.export(), Policy::default(), at(26))
        .unwrap()
        .book;
    let refused = book.apply(&who("d"), Intent::RequestRelease { task: task(3) }, at(26));
    assert_eq!(refused.outcome, Err(Rejection::RequestPending));
    let answered = book_ok(book.apply(
        &who("a"),
        Intent::AnswerRelease {
            task: task(3),
            requester: member("b"),
            accept: false,
        },
        at(27),
    ));
    assert_eq!(book_kinds(&answered), vec!["answered"]);
    // d still cools down until minute 28.
    let refused = book.apply(&who("d"), Intent::RequestRelease { task: task(3) }, at(27));
    assert_eq!(refused.outcome, Err(Rejection::Cooldown { until: at(28) }));
}

#[test]
fn a_request_that_ran_out_during_the_downtime_ends_at_the_restore() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book_ok(book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(1)));
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(20)).unwrap();
    assert_eq!(
        book_kinds(&restored.events),
        vec!["reserved", "request ended"]
    );
    assert_eq!(
        restored.events[1].1,
        ReservationEvent::ReleaseRequestEnded {
            holder: who("a"),
            requester: who("b"),
            reason: RequestEndReason::TimedOut
        }
    );
    // The cooldown counts from the timeout (minute 11).
    let mut book = restored.book;
    let refused = book.apply(&who("b"), Intent::RequestRelease { task: task(1) }, at(20));
    assert_eq!(refused.outcome, Err(Rejection::Cooldown { until: at(21) }));
}

#[test]
fn an_old_epoch_hold_whose_grace_ran_out_expires_at_the_restore() {
    let mut book = ReservationBook::new(Policy::default(), Epoch(1));
    book.presence(&who("a"), true, at(0));
    book_ok(book.apply(&who("a"), reserve(task(1), 1), at(0)));
    book.rotate(Epoch(2), at(10)).unwrap();
    book_ok(book.apply(
        &who("a"),
        Intent::Heartbeat {
            tasks: vec![task(1)],
        },
        at(19),
    ));
    // Within the grace the hold stays, and the grace still ends at minute 40 (the hold's own
    // deadline is 49, warned at 44).
    let early = ReservationBook::restore(book.export(), Policy::default(), at(20)).unwrap();
    assert_eq!(book_kinds(&early.events), vec!["reserved"]);
    assert_eq!(early.book.next_wakeup(), Some(at(40)));
    let late = ReservationBook::restore(book.export(), Policy::default(), at(42)).unwrap();
    assert_eq!(book_kinds(&late.events), vec!["reserved", "expired"]);
    assert!(late.book.get(&task(1)).is_none());
}

#[test]
fn a_restore_refuses_other_versions_and_broken_snapshots() {
    let good = busy_book().export();
    // Version: in the value, and in stored text even with fields this version does not know.
    let mut other = good.clone();
    other.version = 2;
    assert_eq!(
        ReservationBook::restore(other, Policy::default(), at(31)).unwrap_err(),
        RestoreError::Version {
            got: 2,
            supported: 1
        }
    );
    assert_eq!(
        BookSnapshot::from_json(r#"{"version":2,"shards":[]}"#),
        Err(RestoreError::Version {
            got: 2,
            supported: 1
        })
    );
    assert!(matches!(
        BookSnapshot::from_json(r#"{"version":1,"epoch":1}"#),
        Err(RestoreError::Malformed { .. })
    ));
    assert!(matches!(
        BookSnapshot::from_json("not json"),
        Err(RestoreError::Malformed { .. })
    ));
    let json = serde_json::to_string(&good).unwrap();
    let extra = json.replacen(r#"{"version":1,"#, r#"{"version":1,"extra":0,"#, 1);
    assert!(matches!(
        BookSnapshot::from_json(&extra),
        Err(RestoreError::Malformed { .. })
    ));

    let broken = |change: &dyn Fn(&mut BookSnapshot)| {
        let mut snapshot = good.clone();
        change(&mut snapshot);
        match ReservationBook::restore(snapshot, Policy::default(), at(26)) {
            Err(RestoreError::Inconsistent { problem, .. }) => problem,
            other => panic!("restored: {other:?}"),
        }
    };
    assert_eq!(
        broken(&|s| {
            let first = s.entries[0].clone();
            s.entries.push(first);
        }),
        "the task appears twice"
    );
    assert_eq!(
        broken(&|s| s.entries[0].epoch = Epoch(3)),
        "the hold's epoch is newer than the book's"
    );
    assert_eq!(
        broken(&|s| s.rotated_at = None),
        "a hold of an older epoch without a rotation"
    );
    assert_eq!(
        broken(&|s| s.entries[0].since = at(99)),
        "the hold starts after its last activity"
    );
    assert_eq!(
        broken(&|s| {
            if let Some(request) = &mut s.entries[2].request {
                request.requester = who("a");
            }
        }),
        "the holder asks itself for a release"
    );
    assert_eq!(
        broken(&|s| {
            s.entries[2].since = at(21);
            s.entries[2].last_activity = at(21);
        }),
        "the release request predates the hold"
    );
    assert_eq!(
        broken(&|s| {
            let template = s.entries[0].clone();
            s.entries = (0..=MAX_RESERVATIONS_PER_MEMBER)
                .map(|n| EntrySnapshot {
                    task: KeyedHash::from_bytes([n as u8; 32]),
                    ..template.clone()
                })
                .collect();
        }),
        "more holds of one member than allowed"
    );
    // The error text names the entry by position, never its content.
    let text = RestoreError::Inconsistent {
        index: 2,
        problem: "the task appears twice",
    }
    .to_string();
    assert_eq!(text, "snapshot entry 2: the task appears twice");
}

#[test]
fn an_empty_book_restores_empty() {
    let book = ReservationBook::new(Policy::default(), Epoch(7));
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(0)).unwrap();
    assert!(restored.events.is_empty());
    assert_eq!(restored.book.epoch(), Epoch(7));
    assert_eq!(restored.book.iter().count(), 0);
    assert_eq!(restored.book.next_wakeup(), None);
}

#[test]
fn a_restore_at_the_exact_deadline_warns_instead_of_expiring() {
    // Offline since minute 0, deadline 30; the book was never ticked before the export.
    let book = book_with_offline_hold();
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(30)).unwrap();
    assert_eq!(book_kinds(&restored.events), vec!["reserved", "expiring"]);
    assert_eq!(
        restored.events[1].1,
        ReservationEvent::Expiring {
            holder: who("a"),
            expires_at: at(35)
        }
    );
}

#[test]
fn a_restore_with_the_clock_set_back_keeps_the_stored_deadline() {
    // Offline since minute 0, last activity at 20 (deadline 50); the restore's clock says 10,
    // before the last activity it restores.
    let mut book = book_with_offline_hold();
    book_ok(book.apply(
        &who("a"),
        Intent::Heartbeat {
            tasks: vec![task(1)],
        },
        at(20),
    ));
    let restored = ReservationBook::restore(book.export(), Policy::default(), at(10)).unwrap();
    // max(20 + 30, 10 + 5) is the stored deadline: nothing changes, nothing is reported.
    assert!(restored.events.is_empty());
    assert_eq!(restored.book.next_wakeup(), Some(at(45)));
}

#[test]
fn a_cooldown_that_ends_at_the_restore_is_dropped() {
    let snapshot = busy_book().export();
    // d's cooldown on task 3 runs until minute 28 (inclusive end, as in `cooldown`).
    let restored = ReservationBook::restore(snapshot.clone(), Policy::default(), at(28)).unwrap();
    assert!(restored.book.export().entries[2].cooldowns.is_empty());
    let kept = ReservationBook::restore(snapshot, Policy::default(), at(28).minus(1)).unwrap();
    assert_eq!(kept.book.export().entries[2].cooldowns.len(), 1);
}
