//! Desktop notifications through the freedesktop notification service
//! (`org.freedesktop.Notifications` on the session bus), the way Linux desktops show them.
//!
//! The D-Bus calls sit behind [`Bus`], so the bookkeeping here (one notification per session,
//! click to session, text preparation) is tested without a bus; the real bus is `dbus.rs`.
//! One notification per session: a new one passes the previous server id as `replaces_id`, the
//! freedesktop counterpart of the toast tag on Windows.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{clean_text, NotifyError};

/// The `app_name` the server shows, and the summary when the title is empty after cleaning.
pub const APP_NAME: &str = "Hangar";

/// The action key the specification reserves for a click on the notification itself.
pub const DEFAULT_ACTION: &str = "default";

/// What the running notification server can do, from `GetCapabilities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// The server reports clicks (`ActionInvoked`); without it a notification only informs.
    pub actions: bool,
    /// The server reads the body as markup (a small HTML subset), so text must be escaped.
    pub body_markup: bool,
}

impl Capabilities {
    pub fn from_names<S: AsRef<str>>(names: &[S]) -> Self {
        let has = |wanted: &str| names.iter().any(|name| name.as_ref() == wanted);
        Self {
            actions: has("actions"),
            body_markup: has("body-markup"),
        }
    }
}

/// One `Notify` call, already cleaned and escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub replaces_id: u32,
    pub summary: String,
    pub body: String,
    /// Pairs of action key and label, flattened as the specification wants them.
    pub actions: Vec<String>,
}

/// Only `&`, `<` and `>` matter for the markup subset: escaped, no tag (`<a href>`, `<img src>`)
/// can come out of a terminal title. Quotes stay, some servers show `&quot;` literally.
pub fn escape_markup(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Title and body as plain text, the body escaped where the server reads markup, and the default
/// action only where the server reports clicks. The summary is plain text by specification.
pub fn request(replaces_id: u32, title: &str, body: &str, capabilities: Capabilities) -> Request {
    let summary = clean_text(title);
    let body = clean_text(body);
    Request {
        replaces_id,
        summary: if summary.is_empty() {
            APP_NAME.to_owned()
        } else {
            summary
        },
        body: if capabilities.body_markup {
            escape_markup(&body)
        } else {
            body
        },
        // The label is rarely shown (a click on the notification invokes it); the product name
        // needs no translation.
        actions: if capabilities.actions {
            vec![DEFAULT_ACTION.to_owned(), APP_NAME.to_owned()]
        } else {
            Vec::new()
        },
    }
}

/// The notification service on the session bus.
pub trait Bus: Send + Sync {
    fn capabilities(&self) -> Result<Vec<String>, NotifyError>;
    /// Shows or replaces a notification; the server's id for it.
    fn notify(&self, request: &Request) -> Result<u32, NotifyError>;
    fn close(&self, id: u32) -> Result<(), NotifyError>;
}

/// What the bus's signal threads report, in the order they saw it. `sender` is the unique bus name
/// in the signal's header: only the current owner of `org.freedesktop.Notifications` is believed,
/// because any program on the session bus can send a signal straight to Hangar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The notification server's unique name changed (`None`: no server any more).
    OwnerChanged(Option<String>),
    ActionInvoked {
        sender: Option<String>,
        id: u32,
        action: String,
    },
    NotificationClosed {
        sender: Option<String>,
        id: u32,
    },
}

/// Receives the [`Event`]s on a dispatch thread of the bus, never on the thread that reads the
/// socket, so a slow click handler cannot hold up method replies.
pub type EventHandler = Arc<dyn Fn(Event) + Send + Sync>;

/// Opens the bus and subscribes the handler to its signals, before the first notification.
pub type Connector = Box<dyn Fn(EventHandler) -> Result<Arc<dyn Bus>, NotifyError> + Send + Sync>;

type OnClick = Arc<dyn Fn() + Send + Sync>;

struct Shown {
    id: u32,
    on_click: Option<OnClick>,
}

#[derive(Default)]
struct State {
    /// Unique name of the notification server the ids below belong to.
    owner: Option<String>,
    /// The last notification per session with its click handler.
    shown: HashMap<u64, Shown>,
}

impl State {
    fn is_from_server(&self, sender: Option<&String>) -> bool {
        sender.is_some() && sender == self.owner.as_ref()
    }

    /// The click handler an event asks for, if any; it runs once the lock is released.
    fn apply(&mut self, event: Event) -> Option<OnClick> {
        match event {
            Event::OwnerChanged(owner) => {
                // Ids belong to the server that issued them: after a restart or a switch to
                // another server they could name someone else's notification. Unknown to known
                // (the first answer, or a server started on demand) keeps them.
                if self.owner.is_some() && self.owner != owner {
                    self.shown.clear();
                }
                self.owner = owner;
                None
            }
            Event::ActionInvoked { sender, id, action } => {
                if action != DEFAULT_ACTION || !self.is_from_server(sender.as_ref()) {
                    return None;
                }
                self.shown
                    .values()
                    .find(|shown| shown.id == id)
                    .and_then(|shown| shown.on_click.clone())
            }
            Event::NotificationClosed { sender, id } => {
                if self.is_from_server(sender.as_ref()) {
                    self.shown.retain(|_, shown| shown.id != id);
                }
                None
            }
        }
    }
}

pub struct Notifier {
    connector: Connector,
    /// Opened with the first notification and kept: it carries the signals, and a server that
    /// starts later is reached through the same connection.
    bus: Mutex<Option<Arc<dyn Bus>>>,
    /// Held only while connecting, so concurrent notifications open one connection while
    /// `forget` (which takes only `bus`) never waits for a connection attempt.
    connecting: Mutex<()>,
    state: Arc<Mutex<State>>,
}

impl Notifier {
    pub fn with_connector(connector: Connector) -> Self {
        Self {
            connector,
            bus: Mutex::new(None),
            connecting: Mutex::new(()),
            state: Arc::default(),
        }
    }

    fn opened(&self) -> Option<Arc<dyn Bus>> {
        crate::lock(&self.bus).clone()
    }

    fn bus(&self) -> Result<Arc<dyn Bus>, NotifyError> {
        if let Some(bus) = self.opened() {
            return Ok(bus);
        }
        let _connecting = crate::lock(&self.connecting);
        if let Some(bus) = self.opened() {
            return Ok(bus);
        }
        let state = Arc::clone(&self.state);
        let handler: EventHandler = Arc::new(move |event| {
            let on_click = crate::lock(&state).apply(event);
            if let Some(on_click) = on_click {
                on_click();
            }
        });
        let opened = (self.connector)(handler)?;
        *crate::lock(&self.bus) = Some(Arc::clone(&opened));
        Ok(opened)
    }

    /// `Ok(true)` once the server took the notification; an error when there is no session bus or
    /// no notification server. `on_click` runs on the bus's dispatch thread, and only where the
    /// server reports clicks.
    pub fn show(
        &self,
        _app_id: &str,
        session_id: u64,
        title: &str,
        body: &str,
        on_click: impl Fn() + Send + Sync + 'static,
    ) -> Result<bool, NotifyError> {
        let bus = self.bus()?;
        // Asked every time: the server can change while Hangar runs (one started later, another
        // desktop's), and the escaping must match the one that shows the text.
        let capabilities = Capabilities::from_names(&bus.capabilities()?);
        let replaces_id = crate::lock(&self.state)
            .shown
            .get(&session_id)
            .map_or(0, |shown| shown.id);
        let id = bus.notify(&request(replaces_id, title, body, capabilities))?;
        let on_click: Option<OnClick> = capabilities.actions.then(|| Arc::new(on_click) as OnClick);
        crate::lock(&self.state)
            .shown
            .insert(session_id, Shown { id, on_click });
        Ok(true)
    }

    /// The session is closed: drop its click handler and close its notification, best effort.
    pub fn forget(&self, session_id: u64) {
        let Some(shown) = crate::lock(&self.state).shown.remove(&session_id) else {
            return;
        };
        let Some(bus) = self.opened() else {
            return;
        };
        if let Err(error) = bus.close(shown.id) {
            eprintln!("hangar: notification of session {session_id} not withdrawn: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const ALL: Capabilities = Capabilities {
        actions: true,
        body_markup: true,
    };

    #[test]
    fn capabilities_come_from_the_names() {
        assert_eq!(
            Capabilities::from_names(&["body", "actions", "body-markup", "persistence"]),
            ALL
        );
        assert_eq!(Capabilities::from_names(&["body"]), Capabilities::default());
    }

    #[test]
    fn markup_cannot_be_injected() {
        let made = request(0, "<b>x</b>", "</a><img src='x'/> & more \"q\"", ALL);
        assert_eq!(made.summary, "<b>x</b>", "the summary is plain text");
        assert_eq!(made.body, "&lt;/a&gt;&lt;img src='x'/&gt; &amp; more \"q\"");
        let plain = request(0, "t", "a < b & c", Capabilities::default());
        assert_eq!(plain.body, "a < b & c", "no markup, nothing to escape");
    }

    #[test]
    fn texts_are_cleaned_and_an_empty_title_names_the_app() {
        let made = request(3, " \x1b[1m\u{202E}", "line\r\nnext\u{200B}", ALL);
        assert_eq!(made.summary, "[1m");
        assert_eq!(made.body, "line  next");
        assert_eq!(made.replaces_id, 3);
        assert_eq!(request(0, "\u{200B}", "", ALL).summary, APP_NAME);
        assert_eq!(
            request(0, &"ä".repeat(500), "", ALL)
                .summary
                .chars()
                .count(),
            super::super::MAX_CHARS
        );
    }

    #[test]
    fn the_default_action_only_where_clicks_are_reported() {
        assert_eq!(request(0, "t", "b", ALL).actions, ["default", "Hangar"]);
        assert!(request(0, "t", "b", Capabilities::default())
            .actions
            .is_empty());
    }

    #[derive(Default)]
    struct FakeBus {
        capabilities: Vec<&'static str>,
        fail: bool,
        requests: Mutex<Vec<Request>>,
        closed: Mutex<Vec<u32>>,
        next_id: Mutex<u32>,
    }

    impl Bus for FakeBus {
        fn capabilities(&self) -> Result<Vec<String>, NotifyError> {
            if self.fail {
                return Err(NotifyError::Service("no server".to_owned()));
            }
            Ok(self.capabilities.iter().map(|s| (*s).to_owned()).collect())
        }

        fn notify(&self, request: &Request) -> Result<u32, NotifyError> {
            self.requests.lock().unwrap().push(request.clone());
            let mut next = self.next_id.lock().unwrap();
            // Like a real server: a known replaces_id keeps its id, otherwise a new one.
            if request.replaces_id != 0 {
                return Ok(request.replaces_id);
            }
            *next += 1;
            Ok(*next + 100)
        }

        fn close(&self, id: u32) -> Result<(), NotifyError> {
            self.closed.lock().unwrap().push(id);
            Ok(())
        }
    }

    /// The unique name of the fake server, and of another program on the bus.
    const SERVER: &str = ":1.5";
    const INTRUDER: &str = ":1.66";

    struct Harness {
        notifier: Notifier,
        bus: Arc<FakeBus>,
        handler: Arc<Mutex<Option<EventHandler>>>,
        connects: Arc<AtomicUsize>,
    }

    fn harness(bus: FakeBus) -> Harness {
        let bus = Arc::new(bus);
        let handler: Arc<Mutex<Option<EventHandler>>> = Arc::default();
        let connects = Arc::new(AtomicUsize::new(0));
        let (b, h, c) = (
            Arc::clone(&bus),
            Arc::clone(&handler),
            Arc::clone(&connects),
        );
        let notifier = Notifier::with_connector(Box::new(move |on_event| {
            c.fetch_add(1, Ordering::SeqCst);
            // Like `dbus::connect`: the current owner is the first event.
            on_event(Event::OwnerChanged(Some(SERVER.to_owned())));
            *h.lock().unwrap() = Some(on_event);
            Ok(Arc::clone(&b) as Arc<dyn Bus>)
        }));
        Harness {
            notifier,
            bus,
            handler,
            connects,
        }
    }

    fn send(harness: &Harness, event: Event) {
        let handler = harness.handler.lock().unwrap().clone().unwrap();
        handler(event);
    }

    fn click_from(harness: &Harness, sender: &str, id: u32, action: &str) {
        send(
            harness,
            Event::ActionInvoked {
                sender: Some(sender.to_owned()),
                id,
                action: action.to_owned(),
            },
        );
    }

    fn click(harness: &Harness, id: u32, action: &str) {
        click_from(harness, SERVER, id, action);
    }

    fn closed_from(harness: &Harness, sender: &str, id: u32) {
        send(
            harness,
            Event::NotificationClosed {
                sender: Some(sender.to_owned()),
                id,
            },
        );
    }

    fn last_replaces_id(harness: &Harness) -> u32 {
        harness
            .bus
            .requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .replaces_id
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
        let count = Arc::new(AtomicUsize::new(0));
        let inner = Arc::clone(&count);
        (count, move || {
            inner.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn one_notification_per_session_and_a_click_reaches_its_session() {
        let h = harness(FakeBus {
            capabilities: vec!["actions", "body"],
            ..FakeBus::default()
        });
        let (first, on_first) = counter();
        let (second, on_second) = counter();
        assert!(h.notifier.show("", 1, "a", "b", on_first).unwrap());
        assert!(h.notifier.show("", 2, "c", "d", on_second).unwrap());
        let (again, on_again) = counter();
        assert!(h.notifier.show("", 1, "e", "f", on_again).unwrap());
        let replaces: Vec<u32> = h
            .bus
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.replaces_id)
            .collect();
        assert_eq!(
            replaces,
            [0, 0, 101],
            "session 1 replaces its own notification"
        );
        assert_eq!(h.connects.load(Ordering::SeqCst), 1, "one connection");

        click(&h, 101, DEFAULT_ACTION);
        assert_eq!(
            again.load(Ordering::SeqCst),
            1,
            "the newest handler of session 1"
        );
        assert_eq!(first.load(Ordering::SeqCst), 0);
        click(&h, 102, "other-action");
        click(&h, 999, DEFAULT_ACTION);
        assert_eq!(second.load(Ordering::SeqCst), 0, "only the default action");
    }

    #[test]
    fn without_actions_the_notification_only_informs() {
        let h = harness(FakeBus {
            capabilities: vec!["body"],
            ..FakeBus::default()
        });
        let (clicks, on_click) = counter();
        assert!(h.notifier.show("", 1, "a", "b", on_click).unwrap());
        assert!(h.bus.requests.lock().unwrap()[0].actions.is_empty());
        click(&h, 101, DEFAULT_ACTION);
        assert_eq!(clicks.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn closing_a_session_closes_its_notification() {
        let h = harness(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        h.notifier.forget(1);
        assert_eq!(
            h.connects.load(Ordering::SeqCst),
            0,
            "nothing shown, no bus"
        );
        let (clicks, on_click) = counter();
        h.notifier.show("", 1, "a", "b", on_click).unwrap();
        h.notifier.forget(1);
        h.notifier.forget(1);
        assert_eq!(*h.bus.closed.lock().unwrap(), [101]);
        click(&h, 101, DEFAULT_ACTION);
        assert_eq!(clicks.load(Ordering::SeqCst), 0, "the handler is gone");
    }

    #[test]
    fn no_server_is_an_error_not_a_panic() {
        let h = harness(FakeBus {
            fail: true,
            ..FakeBus::default()
        });
        assert!(h.notifier.show("", 1, "a", "b", || {}).is_err());
        assert!(h.bus.requests.lock().unwrap().is_empty());
        let none = Notifier::with_connector(Box::new(|_| {
            Err(NotifyError::Service("no session bus".to_owned()))
        }));
        assert!(none.show("", 1, "a", "b", || {}).is_err());
        assert!(none.show("", 1, "a", "b", || {}).is_err(), "tried again");
        none.forget(1);
    }

    #[test]
    fn signals_from_anyone_but_the_server_are_ignored() {
        let h = harness(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        let (clicks, on_click) = counter();
        h.notifier.show("", 1, "a", "b", on_click).unwrap();
        click_from(&h, INTRUDER, 101, DEFAULT_ACTION);
        send(
            &h,
            Event::ActionInvoked {
                sender: None,
                id: 101,
                action: DEFAULT_ACTION.to_owned(),
            },
        );
        closed_from(&h, INTRUDER, 101);
        assert_eq!(clicks.load(Ordering::SeqCst), 0, "forged clicks");
        h.notifier.show("", 1, "a", "b", || {}).unwrap();
        assert_eq!(last_replaces_id(&h), 101, "a forged close changes nothing");
    }

    #[test]
    fn a_closed_notification_is_not_replaced_or_clicked_later() {
        let h = harness(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        let (clicks, on_click) = counter();
        h.notifier.show("", 1, "a", "b", on_click).unwrap();
        closed_from(&h, SERVER, 101);
        click(&h, 101, DEFAULT_ACTION);
        assert_eq!(clicks.load(Ordering::SeqCst), 0);
        h.notifier.show("", 1, "a", "b", || {}).unwrap();
        assert_eq!(last_replaces_id(&h), 0, "the server may reuse the id");
        h.notifier.forget(1);
        assert_eq!(*h.bus.closed.lock().unwrap(), [102]);
    }

    #[test]
    fn a_new_server_drops_the_ids_of_the_old_one() {
        let h = harness(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        let (clicks, on_click) = counter();
        h.notifier.show("", 1, "a", "b", on_click).unwrap();
        send(&h, Event::OwnerChanged(Some(":1.9".to_owned())));
        click_from(&h, ":1.9", 101, DEFAULT_ACTION);
        assert_eq!(clicks.load(Ordering::SeqCst), 0, "101 was the old server's");
        h.notifier.show("", 1, "a", "b", || {}).unwrap();
        assert_eq!(last_replaces_id(&h), 0);

        // Gone and back (a restart): the ids are dropped as well.
        let (later, on_later) = counter();
        h.notifier.show("", 2, "a", "b", on_later).unwrap();
        send(&h, Event::OwnerChanged(None));
        send(&h, Event::OwnerChanged(Some(":1.9".to_owned())));
        click_from(&h, ":1.9", 103, DEFAULT_ACTION);
        assert_eq!(later.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_server_that_appears_later_keeps_the_ids() {
        let h = harness(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        h.notifier.show("", 9, "a", "b", || {}).unwrap();
        send(&h, Event::OwnerChanged(None));
        // Unknown to known, as when the bus starts the server on demand for the first call.
        let (clicks, on_click) = counter();
        h.notifier.show("", 1, "a", "b", on_click).unwrap();
        send(&h, Event::OwnerChanged(Some(SERVER.to_owned())));
        click(&h, 102, DEFAULT_ACTION);
        assert_eq!(clicks.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn connecting_holds_up_neither_forget_nor_a_second_connection() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let entered = Mutex::new(entered_tx);
        let release = Mutex::new(release_rx);
        let connects = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&connects);
        let bus: Arc<dyn Bus> = Arc::new(FakeBus {
            capabilities: vec!["actions"],
            ..FakeBus::default()
        });
        let notifier = Arc::new(Notifier::with_connector(Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            entered.lock().unwrap().send(()).unwrap();
            release.lock().unwrap().recv().unwrap();
            Ok(Arc::clone(&bus))
        })));
        let shows: Vec<_> = (1..=2)
            .map(|session| {
                let notifier = Arc::clone(&notifier);
                std::thread::spawn(move || notifier.show("", session, "a", "b", || {}).unwrap())
            })
            .collect();
        entered_rx.recv().unwrap();
        // The connector is blocked; forget must still return.
        notifier.forget(1);
        release_tx.send(()).unwrap();
        for show in shows {
            assert!(show.join().unwrap());
        }
        assert_eq!(connects.load(Ordering::SeqCst), 1);
    }
}
