use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

use latch_core::{BrowserContextId, ProcessId, SessionId, TabId, TerminalId, WorkspaceId};
use serde::{Deserialize, Serialize};

const SESSION_FILE: &str = "sessions.json";
const MAX_SESSIONS: usize = 64;
const SESSION_TTL_MS: u64 = 12 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Degraded,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub state: SessionState,
    pub workspace_ids: Vec<WorkspaceId>,
    pub terminal_ids: Vec<TerminalId>,
    pub process_ids: Vec<ProcessId>,
    pub browser_context_ids: Vec<BrowserContextId>,
    pub tab_ids: Vec<TabId>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub resource_generation: u64,
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    session_id: SessionId,
    state: SessionState,
    #[serde(default)]
    workspace_ids: HashSet<WorkspaceId>,
    #[serde(default)]
    terminal_ids: HashSet<TerminalId>,
    #[serde(default)]
    process_ids: HashSet<ProcessId>,
    #[serde(default)]
    browser_context_ids: HashSet<BrowserContextId>,
    #[serde(default)]
    tab_ids: HashSet<TabId>,
    created_at_ms: u64,
    updated_at_ms: u64,
    #[serde(default)]
    resource_generation: u64,
    #[serde(default)]
    degraded_reason: Option<String>,
}

pub struct SessionManager {
    state_path: PathBuf,
    sessions: Mutex<HashMap<SessionId, SessionRecord>>,
}

impl SessionManager {
    pub fn new(state_directory: &Path) -> Self {
        let state_path = state_directory.join(SESSION_FILE);
        let mut sessions = load_sessions(&state_path).unwrap_or_default();
        let now = now_ms();
        sessions.retain(|_, session| {
            session.state != SessionState::Closed
                && now.saturating_sub(session.updated_at_ms) <= SESSION_TTL_MS
        });
        for session in sessions.values_mut() {
            if session.state == SessionState::Active {
                session.state = SessionState::Degraded;
                session.resource_generation = session.resource_generation.saturating_add(1);
                session.terminal_ids.clear();
                session.process_ids.clear();
                session.browser_context_ids.clear();
                session.tab_ids.clear();
                session.degraded_reason = Some(
                    "Latch worker restarted; volatile terminals, processes, browser tabs, and UI references were lost"
                        .to_owned(),
                );
            }
        }
        let manager = Self {
            state_path,
            sessions: Mutex::new(sessions),
        };
        let _ = manager.persist();
        manager
    }

    pub fn create(&self) -> Result<SessionSnapshot, String> {
        self.cleanup();
        let mut sessions = lock(&self.sessions);
        if sessions.len() >= MAX_SESSIONS {
            return Err(format!("at most {MAX_SESSIONS} active sessions are allowed"));
        }
        let now = now_ms();
        let record = SessionRecord {
            session_id: SessionId::new(),
            state: SessionState::Active,
            workspace_ids: HashSet::new(),
            terminal_ids: HashSet::new(),
            process_ids: HashSet::new(),
            browser_context_ids: HashSet::new(),
            tab_ids: HashSet::new(),
            created_at_ms: now,
            updated_at_ms: now,
            resource_generation: 1,
            degraded_reason: None,
        };
        let snapshot = snapshot(&record);
        sessions.insert(record.session_id, record);
        drop(sessions);
        self.persist()?;
        Ok(snapshot)
    }

    pub fn inspect(&self, session_id: SessionId) -> Result<SessionSnapshot, String> {
        self.cleanup();
        let sessions = lock(&self.sessions);
        let record = sessions
            .get(&session_id)
            .ok_or_else(|| "session was not found or expired".to_owned())?;
        Ok(snapshot(record))
    }

    pub fn require_active(&self, session_id: SessionId) -> Result<(), String> {
        let record = self.inspect(session_id)?;
        match record.state {
            SessionState::Active => Ok(()),
            SessionState::Degraded => Err(record
                .degraded_reason
                .unwrap_or_else(|| "session is degraded after worker restart".to_owned())),
            SessionState::Closed => Err("session is closed".to_owned()),
        }
    }

    pub fn reactivate(&self, session_id: SessionId) -> Result<SessionSnapshot, String> {
        let mut sessions = lock(&self.sessions);
        let record = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session was not found or expired".to_owned())?;
        if record.state == SessionState::Closed {
            return Err("session is closed".to_owned());
        }
        record.state = SessionState::Active;
        record.updated_at_ms = now_ms();
        record.degraded_reason = None;
        let result = snapshot(record);
        drop(sessions);
        self.persist()?;
        Ok(result)
    }

    pub fn bind_workspace(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
    ) -> Result<(), String> {
        self.bind(session_id, |session| {
            session.workspace_ids.insert(workspace_id);
        })
    }

    pub fn bind_terminal(
        &self,
        session_id: SessionId,
        terminal_id: TerminalId,
    ) -> Result<(), String> {
        self.bind(session_id, |session| {
            session.terminal_ids.insert(terminal_id);
        })
    }

    pub fn bind_process(
        &self,
        session_id: SessionId,
        process_id: ProcessId,
    ) -> Result<(), String> {
        self.bind(session_id, |session| {
            session.process_ids.insert(process_id);
        })
    }

    pub fn bind_context(
        &self,
        session_id: SessionId,
        context_id: BrowserContextId,
    ) -> Result<(), String> {
        self.bind(session_id, |session| {
            session.browser_context_ids.insert(context_id);
        })
    }

    pub fn bind_tab(&self, session_id: SessionId, tab_id: TabId) -> Result<(), String> {
        self.bind(session_id, |session| {
            session.tab_ids.insert(tab_id);
        })
    }

    pub fn owns_workspace(&self, session_id: SessionId, workspace_id: WorkspaceId) -> bool {
        self.owns(session_id, |session| session.workspace_ids.contains(&workspace_id))
    }

    pub fn owns_terminal(&self, session_id: SessionId, terminal_id: TerminalId) -> bool {
        self.owns(session_id, |session| session.terminal_ids.contains(&terminal_id))
    }

    pub fn owns_process(&self, session_id: SessionId, process_id: ProcessId) -> bool {
        self.owns(session_id, |session| session.process_ids.contains(&process_id))
    }

    pub fn owns_context(&self, session_id: SessionId, context_id: BrowserContextId) -> bool {
        self.owns(session_id, |session| {
            session.browser_context_ids.contains(&context_id)
        })
    }

    pub fn owns_tab(&self, session_id: SessionId, tab_id: TabId) -> bool {
        self.owns(session_id, |session| session.tab_ids.contains(&tab_id))
    }

    pub fn close(&self, session_id: SessionId) -> Result<SessionSnapshot, String> {
        let mut sessions = lock(&self.sessions);
        let record = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session was not found or expired".to_owned())?;
        record.state = SessionState::Closed;
        record.updated_at_ms = now_ms();
        let result = snapshot(record);
        drop(sessions);
        self.persist()?;
        Ok(result)
    }

    pub fn remove(&self, session_id: SessionId) -> Result<(), String> {
        lock(&self.sessions).remove(&session_id);
        self.persist()
    }

    fn bind(&self, session_id: SessionId, apply: impl FnOnce(&mut SessionRecord)) -> Result<(), String> {
        let mut sessions = lock(&self.sessions);
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| "session was not found or expired".to_owned())?;
        if session.state != SessionState::Active {
            return Err("session is not active".to_owned());
        }
        apply(session);
        session.updated_at_ms = now_ms();
        drop(sessions);
        self.persist()
    }

    fn owns(&self, session_id: SessionId, check: impl FnOnce(&SessionRecord) -> bool) -> bool {
        lock(&self.sessions)
            .get(&session_id)
            .is_some_and(|session| session.state == SessionState::Active && check(session))
    }

    fn cleanup(&self) {
        let now = now_ms();
        let mut sessions = lock(&self.sessions);
        let before = sessions.len();
        sessions.retain(|_, session| {
            session.state != SessionState::Closed
                && now.saturating_sub(session.updated_at_ms) <= SESSION_TTL_MS
        });
        let changed = before != sessions.len();
        drop(sessions);
        if changed {
            let _ = self.persist();
        }
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let sessions = lock(&self.sessions).values().cloned().collect::<Vec<_>>();
        let bytes = serde_json::to_vec_pretty(&sessions).map_err(|error| error.to_string())?;
        fs::write(&self.state_path, bytes).map_err(|error| error.to_string())
    }
}

fn load_sessions(path: &Path) -> Result<HashMap<SessionId, SessionRecord>, io::Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let records: Vec<SessionRecord> = serde_json::from_slice(&bytes).unwrap_or_default();
    Ok(records
        .into_iter()
        .map(|record| (record.session_id, record))
        .collect())
}

fn snapshot(record: &SessionRecord) -> SessionSnapshot {
    let mut workspace_ids = record.workspace_ids.iter().copied().collect::<Vec<_>>();
    let mut terminal_ids = record.terminal_ids.iter().copied().collect::<Vec<_>>();
    let mut process_ids = record.process_ids.iter().copied().collect::<Vec<_>>();
    let mut browser_context_ids = record
        .browser_context_ids
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let mut tab_ids = record.tab_ids.iter().copied().collect::<Vec<_>>();
    workspace_ids.sort_by_key(ToString::to_string);
    terminal_ids.sort_by_key(ToString::to_string);
    process_ids.sort_by_key(ToString::to_string);
    browser_context_ids.sort_by_key(ToString::to_string);
    tab_ids.sort_by_key(ToString::to_string);
    SessionSnapshot {
        session_id: record.session_id,
        state: record.state,
        workspace_ids,
        terminal_ids,
        process_ids,
        browser_context_ids,
        tab_ids,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
        resource_generation: record.resource_generation,
        degraded_reason: record.degraded_reason.clone(),
    }
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_bind_resources_and_close() {
        let temp = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(temp.path());
        let session = manager.create().unwrap();
        let workspace = WorkspaceId::new();
        manager.bind_workspace(session.session_id, workspace).unwrap();
        assert!(manager.owns_workspace(session.session_id, workspace));
        let closed = manager.close(session.session_id).unwrap();
        assert_eq!(closed.state, SessionState::Closed);
        assert!(!manager.owns_workspace(session.session_id, workspace));
    }

    #[test]
    fn restart_marks_persisted_active_session_degraded() {
        let temp = tempfile::tempdir().unwrap();
        let session_id = {
            let manager = SessionManager::new(temp.path());
            manager.create().unwrap().session_id
        };
        let manager = SessionManager::new(temp.path());
        let recovered = manager.inspect(session_id).unwrap();
        assert_eq!(recovered.state, SessionState::Degraded);
        assert!(recovered.degraded_reason.is_some());
    }
}
