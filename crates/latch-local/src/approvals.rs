use std::{fs, io, time::{SystemTime, UNIX_EPOCH}};

use latch_core::{ApprovalId, SessionId};
use serde::{Deserialize, Serialize};

use crate::{Capability, LocalError, LocalStore};

const APPROVAL_FILE: &str = "approvals.json";
const MAX_PENDING: usize = 64;
const MAX_GRANTS: usize = 128;
const GRANT_TTL_MS: u64 = 8 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub approval_id: ApprovalId,
    pub session_id: Option<SessionId>,
    pub capability: Capability,
    pub summary: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Deny,
    AllowOnce,
    AllowSession,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ApprovalGrant {
    session_id: Option<SessionId>,
    capability: Capability,
    summary: Option<String>,
    once: bool,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ApprovalState {
    #[serde(default)]
    pending: Vec<ApprovalRequest>,
    #[serde(default)]
    grants: Vec<ApprovalGrant>,
}

impl LocalStore {
    pub fn pending_approvals(&self) -> Result<Vec<ApprovalRequest>, LocalError> {
        let mut state = self.load_approval_state()?;
        prune(&mut state);
        self.save_approval_state(&state)?;
        Ok(state.pending)
    }

    pub fn queue_approval(
        &self,
        session_id: Option<SessionId>,
        capability: Capability,
        summary: &str,
    ) -> Result<ApprovalRequest, LocalError> {
        let mut state = self.load_approval_state()?;
        prune(&mut state);
        if let Some(existing) = state.pending.iter().find(|request| {
            request.session_id == session_id
                && request.capability == capability
                && request.summary == bounded(summary, 240)
        }) {
            return Ok(existing.clone());
        }
        let request = ApprovalRequest {
            approval_id: ApprovalId::new(),
            session_id,
            capability,
            summary: bounded(summary, 240),
            created_at_ms: now_ms(),
        };
        state.pending.push(request.clone());
        if state.pending.len() > MAX_PENDING {
            state.pending.drain(..state.pending.len() - MAX_PENDING);
        }
        self.save_approval_state(&state)?;
        Ok(request)
    }

    pub fn resolve_approval(
        &self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<bool, LocalError> {
        let mut state = self.load_approval_state()?;
        prune(&mut state);
        let Some(index) = state
            .pending
            .iter()
            .position(|request| request.approval_id == approval_id)
        else {
            return Ok(false);
        };
        let request = state.pending.remove(index);
        match decision {
            ApprovalDecision::Deny => {}
            ApprovalDecision::AllowOnce => state.grants.push(ApprovalGrant {
                session_id: request.session_id,
                capability: request.capability,
                summary: Some(request.summary),
                once: true,
                expires_at_ms: now_ms().saturating_add(GRANT_TTL_MS),
            }),
            ApprovalDecision::AllowSession => {
                if let Some(session_id) = request.session_id {
                    state.grants.push(ApprovalGrant {
                        session_id: Some(session_id),
                        capability: request.capability,
                        summary: None,
                        once: false,
                        expires_at_ms: now_ms().saturating_add(GRANT_TTL_MS),
                    });
                } else {
                    state.grants.push(ApprovalGrant {
                        session_id: None,
                        capability: request.capability,
                        summary: Some(request.summary),
                        once: true,
                        expires_at_ms: now_ms().saturating_add(GRANT_TTL_MS),
                    });
                }
            }
        }
        if state.grants.len() > MAX_GRANTS {
            state.grants.drain(..state.grants.len() - MAX_GRANTS);
        }
        self.save_approval_state(&state)?;
        Ok(true)
    }

    pub fn consume_approval_grant(
        &self,
        session_id: Option<SessionId>,
        capability: Capability,
        summary: &str,
    ) -> Result<bool, LocalError> {
        let mut state = self.load_approval_state()?;
        prune(&mut state);
        let summary = bounded(summary, 240);
        let Some(index) = state.grants.iter().position(|grant| {
            grant.capability == capability
                && grant.session_id == session_id
                && grant.summary.as_deref().is_none_or(|expected| expected == summary)
        }) else {
            self.save_approval_state(&state)?;
            return Ok(false);
        };
        if state.grants[index].once {
            state.grants.remove(index);
        }
        self.save_approval_state(&state)?;
        Ok(true)
    }

    pub fn clear_session_approvals(&self, session_id: SessionId) -> Result<(), LocalError> {
        let mut state = self.load_approval_state()?;
        state
            .pending
            .retain(|request| request.session_id != Some(session_id));
        state
            .grants
            .retain(|grant| grant.session_id != Some(session_id));
        self.save_approval_state(&state)
    }

    fn load_approval_state(&self) -> Result<ApprovalState, LocalError> {
        let path = self.directory().join(APPROVAL_FILE);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|source| LocalError::InvalidConfig { path, source }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ApprovalState::default()),
            Err(source) => Err(LocalError::Io { path, source }),
        }
    }

    fn save_approval_state(&self, state: &ApprovalState) -> Result<(), LocalError> {
        fs::create_dir_all(self.directory()).map_err(|source| LocalError::Io {
            path: self.directory().to_path_buf(),
            source,
        })?;
        let path = self.directory().join(APPROVAL_FILE);
        let bytes = serde_json::to_vec_pretty(state).map_err(LocalError::Serialize)?;
        fs::write(&path, bytes).map_err(|source| LocalError::Io { path, source })
    }
}

fn prune(state: &mut ApprovalState) {
    let now = now_ms();
    state.grants.retain(|grant| grant.expires_at_ms > now);
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_grant_is_consumed_and_session_grant_persists() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::new(temp.path());
        let session = SessionId::new();
        let once = store
            .queue_approval(Some(session), Capability::ClipboardWrite, "write clipboard")
            .unwrap();
        store
            .resolve_approval(once.approval_id, ApprovalDecision::AllowOnce)
            .unwrap();
        assert!(store
            .consume_approval_grant(Some(session), Capability::ClipboardWrite, "write clipboard")
            .unwrap());
        assert!(!store
            .consume_approval_grant(Some(session), Capability::ClipboardWrite, "write clipboard")
            .unwrap());

        let persistent = store
            .queue_approval(Some(session), Capability::NativeSystemControl, "volume")
            .unwrap();
        store
            .resolve_approval(persistent.approval_id, ApprovalDecision::AllowSession)
            .unwrap();
        assert!(store
            .consume_approval_grant(Some(session), Capability::NativeSystemControl, "anything")
            .unwrap());
        assert!(store
            .consume_approval_grant(Some(session), Capability::NativeSystemControl, "again")
            .unwrap());
    }
}
