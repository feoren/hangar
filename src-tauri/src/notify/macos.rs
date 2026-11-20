//! The real [`Center`]: `UNUserNotificationCenter` through the objc2 bindings (evidence:
//! docs/wiki/pages/sources/platform-parity-reading.md).
//!
//! The framework answers permission and delivery through completion handlers on a queue of its
//! own; `post` runs on a blocking worker thread (`notify_session`), so it waits for them with a
//! timeout instead of answering before the system did.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

use super::user_notifications::{Center, Clicks, Request};
use super::NotifyError;

/// The first request shows the system's permission dialog, which waits for the person; past this
/// the notification counts as not shown and the window keeps its own signals. The answer still
/// arrives and holds for every later notification.
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(30);
/// Handing a request over is local; past this it counts as failed.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Delegate` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "HangarNotificationDelegate"]
    #[ivars = Clicks]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // A notification while Hangar is the active app is shown too, as on Windows and Linux:
        // the session that waits is usually not the one on screen.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let identifier = response.notification().request().identifier().to_string();
            // SAFETY: a constant string the framework exports; reading it has no side effect.
            let default_action = unsafe { UNNotificationDefaultActionIdentifier };
            let is_default = response.actionIdentifier().isEqualToString(default_action);
            if let Some(on_click) = self.ivars().handler(&identifier, is_default) {
                // Off the framework's queue, like the WinRT thread-pool on Windows: showing the
                // window must not hold up the next response.
                let spawned = std::thread::Builder::new()
                    .name("notification-click".into())
                    .spawn(move || on_click());
                if let Err(error) = spawned {
                    eprintln!("hangar: a notification click was not handled: {error}");
                }
            }
            completion_handler.call(());
        }
    }
);

impl Delegate {
    fn new(clicks: Clicks) -> Retained<Self> {
        let this = Self::alloc().set_ivars(clicks);
        // SAFETY: NSObject's `init` takes no arguments and returns the initialized object.
        unsafe { msg_send![super(this), init] }
    }
}

struct MacCenter {
    /// The center holds its delegate weakly; this keeps it alive for as long as Hangar runs.
    _delegate: Retained<Delegate>,
}

/// The center with Hangar's delegate; `None` outside an app bundle (`tauri dev`, `cargo run`),
/// where the framework raises an Objective-C exception instead of answering. The first access is
/// guarded against that exception as well, in case a bundle the framework does not accept raises
/// it too; later calls get the same shared object. The object itself is not kept: objc2 does not
/// mark it `Send` or `Sync`, and the notifier is shared between threads.
pub fn connect(clicks: Clicks) -> Option<Box<dyn Center>> {
    NSBundle::mainBundle().bundleIdentifier()?;
    let Ok(center) = objc2::exception::catch(UNUserNotificationCenter::currentNotificationCenter)
    else {
        eprintln!("hangar: the notification center is not available");
        return None;
    };
    let delegate = Delegate::new(clicks);
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    Some(Box::new(MacCenter {
        _delegate: delegate,
    }))
}

/// The error's description; `None` for a null error.
fn describe(error: *mut NSError) -> Option<String> {
    // SAFETY: the framework passes null or a valid NSError that lives for the handler's call.
    unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string())
}

fn authorized(center: &UNUserNotificationCenter) -> Result<bool, NotifyError> {
    let (sender, receiver) = mpsc::channel();
    let answer = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        let _ = sender.send(match describe(error) {
            Some(error) => Err(error),
            None => Ok(granted.as_bool()),
        });
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &answer,
    );
    match receiver.recv_timeout(PERMISSION_TIMEOUT) {
        Ok(Ok(granted)) => Ok(granted),
        Ok(Err(error)) => Err(NotifyError::Service(error)),
        Err(_) => Ok(false),
    }
}

// `post` and `remove` run on blocking worker threads, which have no autorelease pool of their own;
// each call gets one, so the framework's temporary objects are freed when it returns.
impl Center for MacCenter {
    fn post(&self, request: &Request) -> Result<bool, NotifyError> {
        objc2::rc::autoreleasepool(|_| post(request))
    }

    fn remove(&self, identifier: &str) {
        objc2::rc::autoreleasepool(|_| remove(identifier));
    }
}

fn post(request: &Request) -> Result<bool, NotifyError> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    if !authorized(&center)? {
        return Ok(false);
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&request.title));
    content.setBody(&NSString::from_str(&request.body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let identifier = NSString::from_str(&request.identifier);
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);
    let (sender, receiver) = mpsc::channel();
    let done = RcBlock::new(move |error: *mut NSError| {
        let _ = sender.send(describe(error));
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
    match receiver.recv_timeout(DELIVERY_TIMEOUT) {
        Ok(None) => Ok(true),
        Ok(Some(error)) => Err(NotifyError::Service(error)),
        Err(_) => Err(NotifyError::Service(
            "no answer from the notification center".to_owned(),
        )),
    }
}

fn remove(identifier: &str) {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let identifiers = NSArray::from_retained_slice(&[NSString::from_str(identifier)]);
    center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
    center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
}
