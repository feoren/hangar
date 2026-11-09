//! Data model of a session and its allowed state transitions.

use std::path::PathBuf;

use serde::Serialize;

pub type SessionId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Starting,
    Running,
    Exited,
}

/// What a session runs for. `update` runs a program's update command (`update_hint`);
/// a restart repeats the update, and the frontend does not keep it in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    #[default]
    Agent,
    Update,
}

/// Whether the agent gate (`gate_service.rs`) watches the session's current process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GateStatus {
    /// Not for this program, the gate was off at the start, or an update session.
    #[default]
    None,
    /// The process got the gate's hooks.
    Active,
    /// The program could take the hooks, but Hangar's local hook endpoint did not start.
    NoEndpoint,
    /// A cmd.exe launcher (npm's `claude.cmd`): the hooks' settings file could not be written to
    /// a path cmd.exe passes on unchanged.
    Launcher,
    /// The start arguments (profile or resume) already pass `--settings`; Hangar adds no second
    /// one ([`crate::manager::has_own_settings`]).
    ProfileSettings,
}

/// What the frontend learns about a session. The profile's environment (`env`) is left out
/// on purpose because it may contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub status: Status,
    pub exit_code: Option<u32>,
    /// Unix time in milliseconds.
    pub created_at: u64,
    /// The current process was started with `resume_args` (conversation resumed). `false` after
    /// a fallback to the normal start or after a restart. This lets the frontend
    /// show whether the conversation carries on or started fresh.
    pub resumed: bool,
    pub kind: SessionKind,
    /// Conversation id Hangar gave the CLI (`session_id_args`, `resume_id_args`), a lowercase
    /// UUID. The frontend stores it in the workspace to resume exactly this conversation. Changes
    /// on a restart and after a resume fallback without an id.
    pub cli_session_id: Option<String>,
    /// `watch_sub_agents` works for this session: the program has a sub-agent source and the
    /// session a conversation id.
    pub sub_agents: bool,
    /// The agent gate for the current process.
    pub gate: GateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Status change from {from:?} to {to:?} is not allowed")]
pub struct TransitionError {
    pub from: Status,
    pub to: Status,
}

impl SessionInfo {
    pub fn new(
        id: SessionId,
        profile_id: String,
        title: String,
        cwd: PathBuf,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            profile_id,
            title,
            cwd,
            status: Status::Starting,
            exit_code: None,
            created_at,
            resumed: false,
            kind: SessionKind::Agent,
            cli_session_id: None,
            sub_agents: false,
            gate: GateStatus::None,
        }
    }

    pub fn mark_running(&mut self) -> Result<(), TransitionError> {
        self.transition(Status::Running, &[Status::Starting])
    }

    /// A process may also end before it was reported as running.
    pub fn mark_exited(&mut self, code: Option<u32>) -> Result<(), TransitionError> {
        self.transition(Status::Exited, &[Status::Starting, Status::Running])?;
        self.exit_code = code;
        Ok(())
    }

    /// Restart is only allowed from an exited state, so two processes never hang on one
    /// session.
    pub fn mark_restarting(&mut self) -> Result<(), TransitionError> {
        self.transition(Status::Starting, &[Status::Exited])?;
        self.exit_code = None;
        Ok(())
    }

    fn transition(&mut self, to: Status, allowed_from: &[Status]) -> Result<(), TransitionError> {
        if !allowed_from.contains(&self.status) {
            return Err(TransitionError {
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SessionInfo {
        SessionInfo::new(1, "pwsh".into(), "PowerShell".into(), PathBuf::from("."), 0)
    }

    #[test]
    fn new_session_is_starting() {
        let s = info();
        assert_eq!(s.status, Status::Starting);
        assert_eq!(s.exit_code, None);
    }

    #[test]
    fn normal_lifecycle() {
        let mut s = info();
        s.mark_running().unwrap();
        s.mark_exited(Some(3)).unwrap();
        assert_eq!(s.status, Status::Exited);
        assert_eq!(s.exit_code, Some(3));
        s.mark_restarting().unwrap();
        assert_eq!(s.status, Status::Starting);
        assert_eq!(s.exit_code, None);
    }

    #[test]
    fn exit_before_running_is_allowed() {
        let mut s = info();
        s.mark_exited(Some(1)).unwrap();
        assert_eq!(s.status, Status::Exited);
    }

    #[test]
    fn forbidden_transitions() {
        let mut s = info();
        assert_eq!(
            s.mark_restarting(),
            Err(TransitionError {
                from: Status::Starting,
                to: Status::Starting
            })
        );
        s.mark_running().unwrap();
        assert!(s.mark_running().is_err());
        assert!(s.mark_restarting().is_err());
        s.mark_exited(None).unwrap();
        assert!(s.mark_exited(None).is_err());
        assert!(s.mark_running().is_err());
    }

    #[test]
    fn serializes_camel_case_without_env() {
        let json = serde_json::to_value(info()).unwrap();
        assert_eq!(json["profileId"], "pwsh");
        assert_eq!(json["status"], "starting");
        assert!(json.get("exitCode").is_some());
        assert_eq!(json["resumed"], false);
        assert_eq!(json["kind"], "agent");
        assert!(json["cliSessionId"].is_null());
        assert_eq!(json["subAgents"], false);
        assert_eq!(json["gate"], "none");
        let mut gated = info();
        gated.gate = GateStatus::NoEndpoint;
        assert_eq!(serde_json::to_value(&gated).unwrap()["gate"], "noEndpoint");
        gated.gate = GateStatus::Launcher;
        assert_eq!(serde_json::to_value(&gated).unwrap()["gate"], "launcher");
        let mut update = info();
        update.kind = SessionKind::Update;
        assert_eq!(serde_json::to_value(update).unwrap()["kind"], "update");
        assert!(json.get("env").is_none());
    }
}
