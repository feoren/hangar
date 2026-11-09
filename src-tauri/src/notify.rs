//! Desktop notifications per session (contract: docs/wiki/pages/syntheses/agent-workflow-features.md).
//!
//! Windows only for now, through the WinRT toast API. Each toast carries the session id as its tag,
//! so a new notification for a session replaces the previous one instead of piling up. macOS and
//! Linux report "not supported" and the window falls back to its own signals.

use std::path::Path;

pub const MAX_CHARS: usize = 200;

/// The AppUserModelID tauri-winrt-notification offers for programs without one of their own.
/// Toasts then name Windows PowerShell as their origin.
pub const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Same rule as tauri-plugin-notification 2.8 (`desktop.rs`): the bundle identifier only for an
/// installed app, the PowerShell id while running from `target/debug` or `target/release`
/// (`tauri dev`, `cargo run`), because only the installer registers the identifier with a Start
/// menu shortcut and Windows drops toasts of an unregistered id. [UNVERIFIED] that the NSIS
/// installer sets this id on its shortcut; observed indirectly through the plugin's choice.
pub fn app_id<'a>(identifier: &'a str, exe_dir: &Path) -> &'a str {
    let dir = exe_dir.to_string_lossy();
    let sep = std::path::MAIN_SEPARATOR;
    let in_target = [
        format!("{sep}target{sep}debug"),
        format!("{sep}target{sep}release"),
    ]
    .iter()
    .any(|suffix| dir.ends_with(suffix.as_str()));
    if in_target {
        POWERSHELL_APP_ID
    } else {
        identifier
    }
}

/// At most [`MAX_CHARS`] characters, without control characters (XML 1.0 does not allow most
/// of them, and a toast with escape sequences from a terminal title looks broken), invisible
/// characters ([`crate::text::is_invisible`]) and the noncharacters U+FFFE and U+FFFF, which
/// XML 1.0 does not allow either.
pub fn clean_text(text: &str) -> String {
    let mut clean: String = text
        .chars()
        .filter(|c| !crate::text::is_invisible(*c))
        .map(|c| match c {
            '\u{FFFE}' | '\u{FFFF}' => '\u{FFFD}',
            '\u{2028}' | '\u{2029}' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    if let Some((index, _)) = clean.char_indices().nth(MAX_CHARS) {
        clean.truncate(index);
    }
    clean.trim().to_owned()
}

pub fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Toast content: title and body as text, nothing that could run or load anything.
pub fn toast_xml(session_id: u64, title: &str, body: &str) -> String {
    format!(
        "<toast launch=\"session-{session_id}\"><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text></binding></visual></toast>",
        escape_xml(&clean_text(title)),
        escape_xml(&clean_text(body)),
    )
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[cfg(windows)]
    #[error("Toast failed: {0}")]
    Toast(#[from] windows::core::Error),
}

pub use imp::Notifier;

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    use super::{toast_xml, NotifyError};

    /// Group of all session toasts; the tag inside it is the session id.
    const GROUP: &str = "sessions";

    /// Keeps the last toast per session (with the app id it was shown under), so its click
    /// handler lives as long as the toast can be clicked, and drops it once a newer toast
    /// replaces it or the session is closed.
    #[derive(Default)]
    pub struct Notifier {
        shown: Mutex<HashMap<u64, (ToastNotification, String)>>,
    }

    impl Notifier {
        /// Shows the toast; `on_click` runs on a WinRT thread-pool thread when the user clicks it.
        pub fn show(
            &self,
            app_id: &str,
            session_id: u64,
            title: &str,
            body: &str,
            on_click: impl Fn() + Send + 'static,
        ) -> Result<bool, NotifyError> {
            let document = XmlDocument::new()?;
            document.LoadXml(&HSTRING::from(toast_xml(session_id, title, body)))?;
            let toast = ToastNotification::CreateToastNotification(&document)?;
            toast.SetTag(&HSTRING::from(session_id.to_string()))?;
            toast.SetGroup(&HSTRING::from(GROUP))?;
            toast.Activated(&TypedEventHandler::new(move |_, _| {
                on_click();
                Ok(())
            }))?;
            // A thread without COM initialization gets the implicit MTA (windows-core retries
            // with CoIncrementMTAUsage), so this works from a blocking worker thread.
            let notifier =
                ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
            notifier.Show(&toast)?;
            crate::lock(&self.shown).insert(session_id, (toast, app_id.to_owned()));
            Ok(true)
        }

        /// The session is closed: drop its toast and its click handler, and take the toast out of
        /// the notification center, where a click would point at a session that no longer
        /// exists. Removing from the history is best effort.
        pub fn forget(&self, session_id: u64) {
            let Some((_toast, app_id)) = crate::lock(&self.shown).remove(&session_id) else {
                return;
            };
            let removed = ToastNotificationManager::History().and_then(|history| {
                history.RemoveGroupedTagWithId(
                    &HSTRING::from(session_id.to_string()),
                    &HSTRING::from(GROUP),
                    &HSTRING::from(app_id),
                )
            });
            if let Err(error) = removed {
                eprintln!("hangar: notification of session {session_id} not withdrawn: {error}");
            }
        }

        #[cfg(test)]
        pub(crate) fn remember(&self, session_id: u64, toast: ToastNotification, app_id: &str) {
            crate::lock(&self.shown).insert(session_id, (toast, app_id.to_owned()));
        }

        #[cfg(test)]
        pub(crate) fn remembered(&self) -> usize {
            crate::lock(&self.shown).len()
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::NotifyError;

    /// A private field instead of a unit struct, so callers build it with `default()` on every
    /// platform (clippy rejects `default()` on a unit struct).
    #[derive(Default)]
    pub struct Notifier {
        _private: (),
    }

    impl Notifier {
        /// Not supported yet: the window keeps its own attention signals.
        pub fn show(
            &self,
            _app_id: &str,
            _session_id: u64,
            _title: &str,
            _body: &str,
            _on_click: impl Fn() + Send + 'static,
        ) -> Result<bool, NotifyError> {
            Ok(false)
        }

        /// Nothing was shown, so nothing to withdraw.
        pub fn forget(&self, _session_id: u64) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_builds_use_the_powershell_id() {
        let sep = std::path::MAIN_SEPARATOR;
        let dev = format!("C:{sep}src{sep}hangar{sep}src-tauri{sep}target{sep}debug");
        let release = format!("C:{sep}src{sep}hangar{sep}src-tauri{sep}target{sep}release");
        let installed = format!("C:{sep}Users{sep}x{sep}AppData{sep}Local{sep}Hangar");
        assert_eq!(
            app_id("com.feoren.hangar", Path::new(&dev)),
            POWERSHELL_APP_ID
        );
        assert_eq!(
            app_id("com.feoren.hangar", Path::new(&release)),
            POWERSHELL_APP_ID
        );
        assert_eq!(
            app_id("com.feoren.hangar", Path::new(&installed)),
            "com.feoren.hangar"
        );
    }

    #[test]
    fn texts_are_short_and_plain() {
        assert_eq!(
            clean_text("  Claude\x1b[31m is\twaiting\r\n"),
            "Claude [31m is waiting"
        );
        let long = "ä".repeat(500);
        assert_eq!(clean_text(&long).chars().count(), MAX_CHARS);
        assert_eq!(clean_text(""), "");
    }

    #[test]
    fn invisible_and_reordering_characters_are_removed() {
        // An override that would show "exe.txt" as "txt.exe", zero-width joiners, BOM, isolates.
        assert_eq!(
            clean_text("a\u{202E}txt.exe\u{202C}b\u{200B}c\u{FEFF}d\u{2066}e\u{2069}"),
            "atxt.exebcde"
        );
        assert_eq!(clean_text("\u{200E}x\u{200F}\u{2060}"), "x");
        // Arabic letter mark, soft hyphen, Mongolian vowel separator, grapheme joiner, tags,
        // variation selectors.
        assert_eq!(
            clean_text("a\u{061C}b\u{00AD}c\u{180E}d\u{034F}e\u{E0041}\u{E007F}f\u{FE0F}g"),
            "abcdefg"
        );
        assert_eq!(clean_text("x\u{FFFE}y\u{FFFF}"), "x\u{FFFD}y\u{FFFD}");
        assert_eq!(clean_text("one\u{2028}two\u{2029}three"), "one two three");
        // Ordinary text outside those ranges stays.
        assert_eq!(
            clean_text("Größe \u{1F600} \u{2010}"),
            "Größe \u{1F600} \u{2010}"
        );
    }

    #[test]
    fn xml_cannot_be_injected() {
        let xml = toast_xml(7, "<b>&\"'", "</text><image src='x'/>");
        assert!(xml.contains("<text>&lt;b&gt;&amp;&quot;&apos;</text>"));
        assert!(xml.contains("<text>&lt;/text&gt;&lt;image src=&apos;x&apos;/&gt;</text>"));
        assert!(xml.contains("launch=\"session-7\""));
        assert_eq!(xml.matches("<text>").count(), 2);
    }

    #[test]
    fn forgetting_an_unknown_session_is_harmless() {
        Notifier::default().forget(42);
    }

    #[cfg(windows)]
    #[test]
    fn closing_a_session_drops_its_toast() {
        use windows::core::HSTRING;
        use windows::UI::Notifications::ToastNotification;
        let document = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(toast_xml(7, "t", "b")))
            .unwrap();
        const TEST_APP_ID: &str = "Hangar.Tests.Unregistered";
        let toast = ToastNotification::CreateToastNotification(&document).unwrap();
        let notifier = Notifier::default();
        notifier.remember(7, toast.clone(), TEST_APP_ID);
        notifier.remember(8, toast, TEST_APP_ID);
        // Never shown, so the history has nothing to withdraw; that must not matter. An app id of
        // its own, so the test cannot withdraw a toast of a Hangar instance running from source.
        notifier.forget(7);
        assert_eq!(notifier.remembered(), 1);
        notifier.forget(8);
        notifier.forget(8);
        assert_eq!(notifier.remembered(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn toast_xml_is_accepted_by_windows() {
        use windows::core::HSTRING;
        let document = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(toast_xml(1, "Title & <more>", "Body\u{1}")))
            .unwrap();
    }
}
