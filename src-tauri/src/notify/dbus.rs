//! The real [`Bus`]: `org.freedesktop.Notifications` on the session bus through zbus (pure Rust,
//! no libdbus), already in the Linux build through keyring and the single-instance plugin.

use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use zbus::blocking::{connection, Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::Value;
use zbus::{MatchRule, Message};

use super::freedesktop::{Bus, Event, EventHandler, Request, APP_NAME};
use super::NotifyError;

const DESTINATION: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
/// The bus itself: name, interface and sender of `NameOwnerChanged`.
const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// A session bus without a notification server may try to start one on the first call; past this
/// the call fails instead of holding the caller up.
const CALL_TIMEOUT: Duration = Duration::from_secs(1);

impl From<zbus::Error> for NotifyError {
    fn from(error: zbus::Error) -> Self {
        NotifyError::Service(error.to_string())
    }
}

struct DbusBus {
    connection: Connection,
}

impl Bus for DbusBus {
    fn capabilities(&self) -> Result<Vec<String>, NotifyError> {
        let reply = self.connection.call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "GetCapabilities",
            &(),
        )?;
        Ok(reply.body().deserialize::<Vec<String>>()?)
    }

    fn notify(&self, request: &Request) -> Result<u32, NotifyError> {
        let hints: HashMap<&str, Value<'_>> = HashMap::new();
        // Expiry -1: the server's default, as for any other application.
        let body = (
            APP_NAME,
            request.replaces_id,
            "",
            request.summary.as_str(),
            request.body.as_str(),
            &request.actions,
            hints,
            -1_i32,
        );
        let reply = self.connection.call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "Notify",
            &body,
        )?;
        Ok(reply.body().deserialize::<u32>()?)
    }

    fn close(&self, id: u32) -> Result<(), NotifyError> {
        self.connection.call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "CloseNotification",
            &(id,),
        )?;
        Ok(())
    }
}

fn spawn(name: &str, run: impl FnOnce() + Send + 'static) -> Result<(), NotifyError> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(run)
        .map(drop)
        .map_err(|error| NotifyError::Service(error.to_string()))
}

fn sender(message: &Message) -> Option<String> {
    message
        .header()
        .sender()
        .map(|name| name.as_str().to_owned())
}

/// The server's signals as [`Event`]s. The sender is passed on unchecked: only the notifier knows
/// the current owner, and checks it.
fn server_event(message: &Message) -> Option<Event> {
    let header = message.header();
    match header.member()?.as_str() {
        "ActionInvoked" => {
            let (id, action) = message.body().deserialize::<(u32, String)>().ok()?;
            Some(Event::ActionInvoked {
                sender: sender(message),
                id,
                action,
            })
        }
        "NotificationClosed" => {
            let (id, _reason) = message.body().deserialize::<(u32, u32)>().ok()?;
            Some(Event::NotificationClosed {
                sender: sender(message),
                id,
            })
        }
        _ => None,
    }
}

/// `NameOwnerChanged` for the notification name, believed only from the bus itself.
fn owner_event(message: &Message) -> Option<Event> {
    if sender(message).as_deref() != Some(BUS) {
        return None;
    }
    let (name, _old, new) = message
        .body()
        .deserialize::<(String, String, String)>()
        .ok()?;
    (name == DESTINATION).then(|| Event::OwnerChanged((!new.is_empty()).then_some(new)))
}

/// Opens the session bus and subscribes to the server's signals and to changes of its owner before
/// anything is shown, so no click is missed. Two reader threads only parse and forward, so their
/// queues never fill up and hold back method replies; one dispatch thread runs the handler (and
/// with it the click handlers) in order. All three end with the connection.
pub fn connect(on_event: EventHandler) -> Result<Arc<dyn Bus>, NotifyError> {
    let connection = connection::Builder::session()?
        .method_timeout(CALL_TIMEOUT)
        .build()?;
    // A sender in a match rule does not help: zbus does not check a well-known sender on its side,
    // and a signal sent straight to Hangar bypasses the bus's matching. The notifier compares
    // every sender with the owner instead.
    let server_rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(INTERFACE)?
        .path(PATH)?
        .build();
    let owner_rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(BUS)?
        .member("NameOwnerChanged")?
        .arg(0, DESTINATION)?
        .build();
    let server_signals = MessageIterator::for_match_rule(server_rule, &connection, Some(64))?;
    let owner_signals = MessageIterator::for_match_rule(owner_rule, &connection, Some(64))?;

    let (events, received) = mpsc::channel::<Event>();
    let forward = events.clone();
    spawn("hangar-notify-signals", move || {
        for event in server_signals.flatten().filter_map(|m| server_event(&m)) {
            if forward.send(event).is_err() {
                break;
            }
        }
    })?;
    let forward = events.clone();
    spawn("hangar-notify-owner", move || {
        for event in owner_signals.flatten().filter_map(|m| owner_event(&m)) {
            if forward.send(event).is_err() {
                break;
            }
        }
    })?;
    spawn("hangar-notify-dispatch", move || {
        for event in received {
            on_event(event);
        }
    })?;

    // After subscribing, so a change in between arrives as a signal after this answer.
    let owner = connection
        .call_method(
            Some(BUS),
            BUS_PATH,
            Some(BUS),
            "GetNameOwner",
            &(DESTINATION,),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<String>().ok());
    // The dispatch thread is alive (it holds the receiver), so this cannot fail.
    let _ = events.send(Event::OwnerChanged(owner));
    Ok(Arc::new(DbusBus { connection }))
}
