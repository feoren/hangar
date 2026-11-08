//! Conversation ids chosen by Hangar for agent CLIs that accept one (`claude --session-id <uuid>`).
//!
//! A fresh start appends `session_id_args` with a new UUID v4; restoring a session that has a
//! stored id replaces `args` with `resume_id_args` and the same id. Sessions without a stored id
//! (workspace files from before 0.3.0) still resume with `resume_args` (`--continue`). Evidence and
//! measurements: docs/wiki/pages/agents/resume.md.

use crate::persist::normalize_key;
use crate::profile::Profile;

/// Stands for the conversation id in `session_id_args` and `resume_id_args`.
pub const PLACEHOLDER: &str = "{id}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Invalid conversation id: expected a UUID")]
pub struct InvalidId;

/// Canonical lowercase UUID, or [`InvalidId`] for anything else (paths, empty strings).
pub fn validate(id: &str) -> Result<String, InvalidId> {
    normalize_key(id).map_err(|_| InvalidId)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Arguments with every `{id}` replaced.
pub fn fill(args: &[String], id: &str) -> Vec<String> {
    args.iter().map(|a| a.replace(PLACEHOLDER, id)).collect()
}

/// How one process of a session starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub args: Vec<String>,
    /// Started with `resume_args` or `resume_id_args`: a quick failure triggers the fallback.
    pub resume: bool,
    /// The conversation id the process runs with, if the profile supports one.
    pub cli_session_id: Option<String>,
}

impl Plan {
    /// A normal start: `args`, plus `session_id_args` with `id` (or a new id) if the profile has
    /// them. `id` is reused after a failed `resume_id_args` attempt: measured, `claude --resume
    /// <unknown id>` exits before it creates anything, so the id is still free.
    pub fn normal(profile: &Profile, id: Option<String>) -> Self {
        let mut args = profile.args.clone();
        let cli_session_id = profile.session_id_args().map(|extra| {
            let id = id.unwrap_or_else(new_id);
            args.extend(fill(extra, &id));
            id
        });
        Self {
            args,
            resume: false,
            cli_session_id,
        }
    }

    /// The first start of a session. `stored` is an already validated id from the workspace.
    pub fn first(profile: &Profile, resume: bool, stored: Option<&str>) -> Self {
        if resume {
            if let (Some(id), Some(args)) = (stored, profile.resume_id_args()) {
                return Self {
                    args: fill(args, id),
                    resume: true,
                    cli_session_id: Some(id.to_owned()),
                };
            }
            if let Some(args) = profile.resume_args() {
                // `--continue` picks the conversation itself; its id is unknown to Hangar.
                return Self {
                    args: args.to_vec(),
                    resume: true,
                    cli_session_id: None,
                };
            }
        }
        // A stored id without `resume_id_args` may already name a conversation: never reuse it.
        Self::normal(profile, None)
    }

    /// The normal start after a failed resume: the same id if the attempt had one.
    pub fn fallback(profile: &Profile, attempted: Option<String>) -> Self {
        Self::normal(profile, attempted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn claude() -> Profile {
        let source = "[[profile]]\nid='claude'\nname='C'\ncommand='claude'\nargs=['--model','x']\n\
                      resume_args=['--continue']\nsession_id_args=['--session-id','{id}']\n\
                      resume_id_args=['--resume','{id}']\n";
        crate::profile::parse(source).unwrap().remove(0)
    }

    fn plain() -> Profile {
        let source =
            "[[profile]]\nid='p'\nname='P'\ncommand='p'\nargs=['-a']\nresume_args=['-r']\n";
        crate::profile::parse(source).unwrap().remove(0)
    }

    #[test]
    fn fresh_start_appends_a_new_uuid() {
        let plan = Plan::first(&claude(), false, None);
        assert!(!plan.resume);
        let id = plan.cli_session_id.clone().unwrap();
        assert_eq!(validate(&id), Ok(id.clone()));
        assert_eq!(plan.args, strings(&["--model", "x", "--session-id", &id]));
        // Every start gets its own id.
        assert_ne!(Plan::first(&claude(), false, None).cli_session_id, Some(id));
    }

    #[test]
    fn restore_with_stored_id_resumes_exactly_that_conversation() {
        let plan = Plan::first(&claude(), true, Some(ID));
        assert_eq!(plan.args, strings(&["--resume", ID]));
        assert!(plan.resume);
        assert_eq!(plan.cli_session_id.as_deref(), Some(ID));
        // The fallback starts fresh under the same id.
        let fallback = Plan::fallback(&claude(), plan.cli_session_id);
        assert_eq!(
            fallback.args,
            strings(&["--model", "x", "--session-id", ID])
        );
        assert!(!fallback.resume);
    }

    #[test]
    fn restore_without_stored_id_continues_and_falls_back_to_a_new_id() {
        let plan = Plan::first(&claude(), true, None);
        assert_eq!(plan.args, strings(&["--continue"]));
        assert!(plan.resume);
        assert_eq!(plan.cli_session_id, None);
        let fallback = Plan::fallback(&claude(), None);
        let id = fallback.cli_session_id.unwrap();
        assert_eq!(
            fallback.args,
            strings(&["--model", "x", "--session-id", &id])
        );
    }

    #[test]
    fn profiles_without_id_args_behave_as_before() {
        assert_eq!(
            Plan::first(&plain(), true, Some(ID)),
            Plan {
                args: strings(&["-r"]),
                resume: true,
                cli_session_id: None
            }
        );
        assert_eq!(
            Plan::first(&plain(), false, Some(ID)),
            Plan {
                args: strings(&["-a"]),
                resume: false,
                cli_session_id: None
            }
        );
        assert_eq!(
            Plan::fallback(&plain(), Some(ID.into())).cli_session_id,
            None
        );
    }

    #[test]
    fn ids_are_validated_and_placeholders_filled() {
        assert_eq!(validate(&ID.to_uppercase()), Ok(ID.to_owned()));
        for bad in [
            "",
            "..",
            "x",
            "0f8fad5b-d9cb-469f-a165-70867728950e/..",
            "../../etc",
        ] {
            assert_eq!(validate(bad), Err(InvalidId), "{bad}");
        }
        assert_eq!(
            fill(&strings(&["--id={id}", "{id}", "x"]), "u"),
            strings(&["--id=u", "u", "x"])
        );
    }
}
