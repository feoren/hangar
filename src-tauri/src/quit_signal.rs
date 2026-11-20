//! SIGTERM and SIGHUP end Hangar the clean way on Linux and macOS (wiki: session-restore, "After a
//! crash").
//!
//! Without a handler the process dies at the signal: on shutdown, on logoff or when a service
//! manager stops it. The exit marker would stay behind (`exit_marker.rs`) and the next start would
//! report a crash, and the sessions would not be ended in order. With the handler the signal asks
//! the app to exit, so `RunEvent::Exit` runs as after Quit.

use std::io;
use std::task::Poll;

use tokio::signal::unix::{signal, Signal, SignalKind};

/// The signal that asked Hangar to quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitSignal {
    Terminate,
    Hangup,
}

/// Both signals, caught from the moment this returns on. Needs a tokio runtime with the signal
/// driver (Tauri's async runtime has it).
pub struct QuitSignals {
    terminate: Signal,
    hangup: Signal,
}

impl QuitSignals {
    pub fn listen() -> io::Result<Self> {
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            hangup: signal(SignalKind::hangup())?,
        })
    }

    /// Waits for the first of the two. `None` only if both streams ended (runtime shutting down).
    pub async fn next(&mut self) -> Option<QuitSignal> {
        // By hand instead of `tokio::select!`, which would need tokio's macros feature.
        std::future::poll_fn(|cx| {
            let terminate = self.terminate.poll_recv(cx);
            if let Poll::Ready(Some(())) = terminate {
                return Poll::Ready(Some(QuitSignal::Terminate));
            }
            let hangup = self.hangup.poll_recv(cx);
            if let Poll::Ready(Some(())) = hangup {
                return Poll::Ready(Some(QuitSignal::Hangup));
            }
            if terminate.is_ready() && hangup.is_ready() {
                Poll::Ready(None)
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

/// Catches both signals on Tauri's async runtime and calls `on_signal` once at the first one.
/// The handlers are in place when this returns; a failure to install them is logged and leaves the
/// default action (the process dies, the next start shows the crash notice).
pub fn watch(on_signal: impl FnOnce(QuitSignal) + Send + 'static) {
    // Installed inside the runtime, which the signal driver needs, but waited for here so the
    // handlers exist before `setup` goes on.
    let listening = tauri::async_runtime::block_on(async { QuitSignals::listen() });
    let mut signals = match listening {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("hangar: SIGTERM and SIGHUP not handled: {error}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        if let Some(signal) = signals.next().await {
            eprintln!("hangar: {signal:?} received, quitting");
            on_signal(signal);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_hangup_and_a_terminate_are_caught_instead_of_ending_the_process() {
        tauri::async_runtime::block_on(async {
            let mut signals = QuitSignals::listen().unwrap();
            for (raw, expected) in [
                (libc::SIGHUP, QuitSignal::Hangup),
                (libc::SIGTERM, QuitSignal::Terminate),
            ] {
                // SAFETY: kill sends a signal to this process, whose handler is installed above,
                // so the default action (ending the test run) does not happen.
                assert_eq!(unsafe { libc::kill(libc::getpid(), raw) }, 0);
                let caught = tokio::time::timeout(Duration::from_secs(5), signals.next()).await;
                assert_eq!(caught, Ok(Some(expected)));
            }
        });
    }
}
