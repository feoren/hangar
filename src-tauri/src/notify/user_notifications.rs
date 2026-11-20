//! Desktop notifications through macOS' notification center (`UNUserNotificationCenter`).
//!
//! The framework calls sit behind [`Center`], so the bookkeeping here (one notification per
//! session, click to session, text preparation) is tested on every platform; the real center is
//! `macos.rs`. One notification per session: every request carries the session's identifier, and
//! a request with the identifier of a delivered one replaces it, the counterpart of the toast tag
//! on Windows and `replaces_id` on Linux.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{clean_text, NotifyError};

/// The title when the session's title is empty after cleaning.
pub const APP_NAME: &str = "Hangar";

/// What a click runs; shared with the center's delegate.
pub type OnClick = Arc<dyn Fn() + Send + Sync>;

/// One notification request, already cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub identifier: String,
    pub title: String,
    pub body: String,
}

/// The notification identifier of a session.
pub fn identifier(session_id: u64) -> String {
    format!("session-{session_id}")
}

/// The session a notification identifier names; `None` for one Hangar did not make.
pub fn session_of(identifier: &str) -> Option<u64> {
    let digits = identifier.strip_prefix("session-")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Title and body as plain text (the center shows no markup), the app name for an empty title.
pub fn request(session_id: u64, title: &str, body: &str) -> Request {
    let title = clean_text(title);
    Request {
        identifier: identifier(session_id),
        title: if title.is_empty() {
            APP_NAME.to_owned()
        } else {
            title
        },
        body: clean_text(body),
    }
}

/// The notification center.
pub trait Center: Send + Sync {
    /// Asks for permission (the system asks the person once, later calls are answered at once)
    /// and hands the request over. `Ok(false)` when notifications are not allowed.
    fn post(&self, request: &Request) -> Result<bool, NotifyError>;
    /// Takes a delivered or pending notification away, best effort.
    fn remove(&self, identifier: &str);
}

/// The click handler of each session with a notification, shared by [`Notifier`] and the
/// center's delegate, which turns a click into the handler to run.
#[derive(Clone, Default)]
pub struct Clicks(Arc<Mutex<HashMap<u64, OnClick>>>);

impl Clicks {
    /// The handler for a response to the notification `identifier`. Only the default action (a
    /// click on the notification itself) asks for the session; dismissing it does not.
    pub fn handler(&self, identifier: &str, default_action: bool) -> Option<OnClick> {
        if !default_action {
            return None;
        }
        let session_id = session_of(identifier)?;
        crate::lock(&self.0).get(&session_id).cloned()
    }

    fn insert(&self, session_id: u64, on_click: OnClick) -> Option<OnClick> {
        crate::lock(&self.0).insert(session_id, on_click)
    }

    fn remove(&self, session_id: u64) -> Option<OnClick> {
        crate::lock(&self.0).remove(&session_id)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        crate::lock(&self.0).len()
    }
}

/// `None` as the center: outside an app bundle, where the framework cannot work.
pub struct Notifier {
    center: Option<Box<dyn Center>>,
    clicks: Clicks,
}

impl Notifier {
    pub fn with_center(center: Option<Box<dyn Center>>, clicks: Clicks) -> Self {
        Self { center, clicks }
    }

    /// Shows the notification; `false` without a center or permission. `on_click` runs on a
    /// thread of its own when the person clicks it.
    pub fn show(
        &self,
        _app_id: &str,
        session_id: u64,
        title: &str,
        body: &str,
        on_click: impl Fn() + Send + Sync + 'static,
    ) -> Result<bool, NotifyError> {
        let Some(center) = &self.center else {
            return Ok(false);
        };
        // Known before the request is handed over, so even an instant click finds its session.
        let previous = self.clicks.insert(session_id, Arc::new(on_click));
        let posted = center.post(&request(session_id, title, body));
        if !matches!(posted, Ok(true)) {
            match previous {
                Some(previous) => self.clicks.insert(session_id, previous),
                None => self.clicks.remove(session_id),
            };
        }
        posted
    }

    /// The session is closed: drop its click handler and take its notification away.
    pub fn forget(&self, session_id: u64) {
        if self.clicks.remove(session_id).is_none() {
            return;
        }
        if let Some(center) = &self.center {
            center.remove(&identifier(session_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeCenter {
        posted: Mutex<Vec<Request>>,
        removed: Mutex<Vec<String>>,
        /// What `post` answers: `None` fails.
        answer: Mutex<Option<bool>>,
    }

    impl Center for Arc<FakeCenter> {
        fn post(&self, request: &Request) -> Result<bool, NotifyError> {
            crate::lock(&self.posted).push(request.clone());
            crate::lock(&self.answer).ok_or_else(|| NotifyError::Service("not allowed".to_owned()))
        }

        fn remove(&self, identifier: &str) {
            crate::lock(&self.removed).push(identifier.to_owned());
        }
    }

    fn harness(answer: Option<bool>) -> (Arc<FakeCenter>, Clicks, Notifier) {
        let center = Arc::new(FakeCenter::default());
        *crate::lock(&center.answer) = answer;
        let clicks = Clicks::default();
        let notifier = Notifier::with_center(Some(Box::new(Arc::clone(&center))), clicks.clone());
        (center, clicks, notifier)
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
        let count = Arc::new(AtomicUsize::new(0));
        let inner = Arc::clone(&count);
        (count, move || {
            inner.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn identifiers_name_the_session() {
        assert_eq!(identifier(7), "session-7");
        assert_eq!(session_of("session-7"), Some(7));
        assert_eq!(session_of(&identifier(u64::MAX)), Some(u64::MAX));
        assert_eq!(session_of("session-"), None);
        assert_eq!(session_of("session-+7"), None);
        assert_eq!(session_of("session-7a"), None);
        assert_eq!(session_of("other-7"), None);
    }

    #[test]
    fn texts_are_cleaned_and_an_empty_title_names_the_app() {
        assert_eq!(
            request(3, "\u{1b}\u{200B}", "Claude\twaits"),
            Request {
                identifier: "session-3".to_owned(),
                title: APP_NAME.to_owned(),
                body: "Claude waits".to_owned(),
            }
        );
        assert_eq!(request(3, "<b>Title</b>", "").title, "<b>Title</b>");
    }

    #[test]
    fn a_click_on_the_notification_runs_its_session_handler() {
        let (center, clicks, notifier) = harness(Some(true));
        let (count, on_click) = counter();
        assert!(notifier.show("", 5, "t", "b", on_click).unwrap());
        assert_eq!(crate::lock(&center.posted)[0].identifier, "session-5");
        clicks.handler("session-5", true).expect("handler")();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(clicks.handler("session-5", false).is_none(), "dismissed");
        assert!(
            clicks.handler("session-6", true).is_none(),
            "another session"
        );
        assert!(clicks.handler("foreign", true).is_none());
    }

    #[test]
    fn a_new_notification_replaces_the_handler_of_the_session() {
        let (_center, clicks, notifier) = harness(Some(true));
        let (first, on_first) = counter();
        let (second, on_second) = counter();
        notifier.show("", 5, "t", "b", on_first).unwrap();
        notifier.show("", 5, "t", "b", on_second).unwrap();
        clicks.handler("session-5", true).expect("handler")();
        assert_eq!(first.load(Ordering::SeqCst), 0);
        assert_eq!(second.load(Ordering::SeqCst), 1);
        assert_eq!(clicks.len(), 1);
    }

    #[test]
    fn without_permission_nothing_is_kept() {
        let (_center, clicks, notifier) = harness(Some(false));
        let (_count, on_click) = counter();
        assert!(!notifier.show("", 5, "t", "b", on_click).unwrap());
        assert_eq!(clicks.len(), 0);
        let (_center, clicks, notifier) = harness(None);
        let (_count, on_click) = counter();
        assert!(notifier.show("", 5, "t", "b", on_click).is_err());
        assert_eq!(clicks.len(), 0);
    }

    #[test]
    fn a_failed_notification_keeps_the_one_shown_before() {
        let (center, clicks, notifier) = harness(Some(true));
        let (first, on_first) = counter();
        notifier.show("", 5, "t", "b", on_first).unwrap();
        *crate::lock(&center.answer) = Some(false);
        let (_second, on_second) = counter();
        assert!(!notifier.show("", 5, "t", "b", on_second).unwrap());
        clicks.handler("session-5", true).expect("handler")();
        assert_eq!(first.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn closing_a_session_takes_its_notification_away() {
        let (center, clicks, notifier) = harness(Some(true));
        let (_count, on_click) = counter();
        notifier.show("", 5, "t", "b", on_click).unwrap();
        notifier.forget(5);
        notifier.forget(5);
        notifier.forget(6);
        assert_eq!(*crate::lock(&center.removed), vec!["session-5".to_owned()]);
        assert_eq!(clicks.len(), 0);
    }

    #[test]
    fn without_a_center_nothing_is_shown() {
        let notifier = Notifier::with_center(None, Clicks::default());
        assert!(!notifier.show("", 1, "t", "b", || {}).unwrap());
        notifier.forget(1);
    }
}
