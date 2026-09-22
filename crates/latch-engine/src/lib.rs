mod events;
mod session;

use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
    thread,
    time::Duration,
};

use latch_browser::{
    BrowserAction, BrowserError, BrowserManager, BrowserProfile, BrowserTarget, BrowserVerification,
};
use latch_computer::{ComputerError, ComputerManager, MouseButton, ScreenshotFormat, ScrollAxis};
use latch_core::{
    ActionId, McpServerId, ProcessId, SessionId, TerminalId, UiRef, Workspace, WorkspaceError,
    WorkspaceId,
};
use latch_exec::{
    run_blocking, CommandSpec, ExecError, ManagedStreamOutput, ProcessManager, ProcessState,
};
use latch_fs::{EntryKind, FsError, Replacement, SearchMatchKind, SearchOptions, WorkspaceFs};
use latch_local::{
    Capability, LocalConfig, LocalError, LocalStore, McpServerConfig, PermissionMode,
};
use latch_mcp_client::{McpClientError, McpManager};
use latch_protocol::{
    ActionOutcome, ActRequest, AgentRequest, BrowserRequest, DirectoryEntryResponse,
    DirectoryResponse, DisplayResponse, DisplaysResponse, EmptyRequest, EntryKindResponse,
    ErrorCode, EventsRequest, ExecDomainRequest, ExecRequest, ExecResponse, FileContentResponse,
    FileStatResponse, FilesRequest, InspectRequest, KeyRequest, McpCallRequest, McpCallResponse,
    McpServerRequest, McpServerResponse, McpServerStatusResponse, McpServersResponse,
    McpToolResponse, McpToolsResponse, MouseButtonRequest, MouseClickRequest, MouseDragRequest,
    MoveRequest, PatchRequest, PathRequest, PointRequest, ProcessPollResponse, ProcessRequest,
    ProcessStartedResponse, ProcessStateResponse, ProcessStdinRequest, ProcessStreamOutputResponse,
    ProtocolError, RawInputRequest, ReadRequest, Request, RequestEnvelope, ResponseEnvelope,
    ResponsePayload, RootResponse, RootsResponse, ScreenshotFormatRequest, ScreenshotRequest,
    ScreenshotResponse, ScrollAxisRequest, ScrollRequest, SearchMatchKindResponse,
    SearchMatchResponse, SearchRequest, SearchResponse, SessionRequest, ToolsRequest, TypeRequest,
    UiActionRequest, VerificationMode, VerifiedActionResponse, WindowRequest, WindowResponse,
    WindowsResponse, WorkspaceOpenRequest, WorkspacePathRequest, WorkspaceResponse, WriteRequest,
    PROTOCOL_VERSION,
};
use latch_terminal::{TerminalError, TerminalManager, TerminalState};
use latch_windows::{UiAction, UiFindQuery, WindowsError, WindowsManager, WindowsNative};
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{error, info, warn};

pub use events::RuntimeEvent;
pub use session::{SessionSnapshot, SessionState};

const DEFAULT_READ_BYTES: usize = 1024 * 1024;
const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const MAX_WRITE_BYTES: usize = 1024 * 1024;
const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_PATH_CHARS: usize = 4096;

pub struct Engine {
    workspaces: RwLock<HashMap<WorkspaceId, Arc<WorkspaceContext>>>,
    processes: ProcessManager,
    computer: Mutex<ComputerManager>,
    terminals: TerminalManager,
    windows: Option<WindowsManager>,
    browser: BrowserManager,
    mcp: McpManager,
    sessions: session::SessionManager,
    events: events::EventBus,
    revision: AtomicU64,
    local: LocalStore,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let local = LocalStore::default_location().unwrap_or_else(|error| {
            warn!(%error, "local state directory unavailable; using process-local fallback");
            LocalStore::new(std::env::temp_dir().join("Latch"))
        });
        Self::with_store(local)
    }

    pub fn with_store(local: LocalStore) -> Self {
        let windows = WindowsManager::new().map_err(|error| {
            warn!(%error, "Windows semantic provider unavailable; raw computer fallback remains available when permitted");
            error
        }).ok();
        let browser = BrowserManager::new(local.directory());
        let sessions = session::SessionManager::new(local.directory());
        Self {
            workspaces: RwLock::new(HashMap::new()),
            processes: ProcessManager::new(),
            computer: Mutex::new(ComputerManager::new()),
            terminals: TerminalManager::new(),
            windows,
            browser,
            mcp: McpManager::new().unwrap_or_default(),
            sessions,
            events: events::EventBus::new(),
            revision: AtomicU64::new(0),
            local,
        }
    }

    pub const fn local_store(&self) -> &LocalStore {
        &self.local
    }

    pub fn handle_envelope(&self, request: RequestEnvelope) -> ResponseEnvelope {
        if request.version != PROTOCOL_VERSION {
            return ResponseEnvelope::error(
                Some(request.id),
                protocol_error(
                    ErrorCode::UnsupportedVersion,
                    format!(
                        "protocol version {} is unsupported; expected {PROTOCOL_VERSION}",
                        request.version
                    ),
                ),
            );
        }
        let id = request.id;
        match self.handle(request.request) {
            Ok(result) => ResponseEnvelope::success(id, result),
            Err(protocol_error) => {
                if protocol_error.code == ErrorCode::Io {
                    error!(code = ?protocol_error.code, "serious local operation error");
                }
                ResponseEnvelope::error(Some(id), protocol_error)
            }
        }
    }

    pub fn handle(&self, request: Request) -> Result<ResponsePayload, ProtocolError> {
        let config = self.local.load().map_err(map_local_error)?;
        if config.paused {
            return Err(protocol_error(
                ErrorCode::RemotePaused,
                "remote access is paused on this computer",
            ));
        }
        match request {
            Request::Agent(request) => self.handle_agent(&config, request),
            Request::RootsList(request) => self.roots_list_legacy(&config, request),
            Request::WorkspaceOpen(request) => self.open_workspace_legacy(&config, request),
            Request::WorkspaceOpenRaw(request) => self.open_raw_workspace_legacy(&config, request),
            Request::FsList(request) => self.list_dir_legacy(&config, request),
            Request::FsStat(request) => self.stat_file_legacy(&config, request),
            Request::FsRead(request) => self.read_file_legacy(&config, request),
            Request::FsWrite(request) => self.write_file_legacy(&config, request),
            Request::FsPatch(request) => self.patch_file_legacy(&config, request),
            Request::FsSearch(request) => self.search_files_legacy(&config, request),
            Request::FsMkdir(request) => self.create_dir_legacy(&config, request),
            Request::FsMove(request) => self.move_file_legacy(&config, request),
            Request::FsDelete(request) => self.delete_file_legacy(&config, request),
            Request::ExecRun(request) => self.run_command_legacy(&config, request),
            Request::ExecStart(request) => self.start_process_legacy(&config, request),
            Request::ExecPoll(request) => self.poll_process_legacy(&config, request),
            Request::ExecStdin(request) => self.process_stdin_legacy(&config, request),
            Request::ExecKill(request) => self.kill_process_legacy(&config, request),
            Request::ComputerDisplays(request) => self.displays_legacy(&config, request),
            Request::ComputerScreenshot(request) => self.screenshot_legacy(&config, request),
            Request::ComputerWindows(request) => self.windows_legacy(&config, request),
            Request::ComputerFocus(request) => self.focus_window_legacy(&config, request),
            Request::ComputerMouseMove(request) => self.mouse_move_legacy(&config, request),
            Request::ComputerMouseClick(request) => self.mouse_click_legacy(&config, request),
            Request::ComputerMouseDrag(request) => self.mouse_drag_legacy(&config, request),
            Request::ComputerScroll(request) => self.scroll_legacy(&config, request),
            Request::ComputerKey(request) => self.key_legacy(&config, request),
            Request::ComputerType(request) => self.type_text_legacy(&config, request),
            Request::McpServers(request) => self.mcp_servers_legacy(&config, request),
            Request::McpTools(request) => self.mcp_tools_legacy(&config, request),
            Request::McpCall(request) => self.mcp_call_legacy(&config, request),
        }
    }

    pub fn shutdown(&self) {
        self.processes.shutdown_all();
        self.terminals.shutdown_all();
        self.browser.shutdown();
        self.mcp.shutdown();
    }

    fn handle_agent(
        &self,
        config: &LocalConfig,
        request: AgentRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        let value = match request {
            AgentRequest::Session(request) => self.agent_session(config, request)?,
            AgentRequest::Inspect(request) => self.agent_inspect(config, request)?,
            AgentRequest::Files(request) => self.agent_files(config, request)?,
            AgentRequest::Exec(request) => self.agent_exec(config, request)?,
            AgentRequest::Act(request) => self.agent_act(config, request)?,
            AgentRequest::Browser(request) => self.agent_browser(config, request)?,
            AgentRequest::Tools(request) => self.agent_tools(config, request)?,
            AgentRequest::Events(request) => self.agent_events(request)?,
        };
        Ok(ResponsePayload::Agent(value))
    }

    fn agent_session(
        &self,
        _config: &LocalConfig,
        request: SessionRequest,
    ) -> Result<Value, ProtocolError> {
        match request {
            SessionRequest::Create => {
                let session = self.sessions.create().map_err(map_session_error)?;
                self.events.publish(
                    session.session_id,
                    "session",
                    "session.created",
                    "Latch session created",
                    None,
                );
                to_value(session)
            }
            SessionRequest::Inspect { session_id } => {
                to_value(self.sessions.inspect(session_id).map_err(map_session_error)?)
            }
            SessionRequest::Update {
                session_id,
                workspace_ids,
            } => {
                if self.sessions.inspect(session_id).map_err(map_session_error)?.state
                    == SessionState::Degraded
                {
                    self.sessions.reactivate(session_id).map_err(map_session_error)?;
                }
                for workspace_id in workspace_ids {
                    self.workspace(_config, workspace_id)?;
                    self.sessions
                        .bind_workspace(session_id, workspace_id)
                        .map_err(map_session_error)?;
                }
                to_value(self.sessions.inspect(session_id).map_err(map_session_error)?)
            }
            SessionRequest::Close { session_id } | SessionRequest::Cancel { session_id } => {
                let snapshot = self.sessions.inspect(session_id).map_err(map_session_error)?;
                self.cleanup_session_resources(&snapshot);
                let closed = self.sessions.close(session_id).map_err(map_session_error)?;
                self.local
                    .clear_session_approvals(session_id)
                    .map_err(map_local_error)?;
                self.events.publish(
                    session_id,
                    "session",
                    "session.closed",
                    "Latch session closed",
                    None,
                );
                to_value(closed)
            }
        }
    }

    fn cleanup_session_resources(&self, snapshot: &SessionSnapshot) {
        for terminal_id in &snapshot.terminal_ids {
            let _ = self.terminals.remove(*terminal_id);
        }
        for process_id in &snapshot.process_ids {
            let _ = self.processes.kill(*process_id);
        }
        for context_id in &snapshot.browser_context_ids {
            let _ = self.browser.close_context(*context_id);
        }
        if let Some(windows) = &self.windows {
            windows.drop_session(snapshot.session_id);
        }
    }

    fn agent_inspect(
        &self,
        config: &LocalConfig,
        request: InspectRequest,
    ) -> Result<Value, ProtocolError> {
        match request {
            InspectRequest::Windows { session_id, limit } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Inspect desktop windows")?;
                let windows = self.windows_provider()?.desktop_windows(session_id, limit.unwrap_or(32))
                    .map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_uia","windows":windows}))
            }
            InspectRequest::ActiveWindow { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Inspect active window")?;
                let window = self.windows_provider()?.active_window(session_id).map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_uia","window":window}))
            }
            InspectRequest::UiTree { session_id, root, depth, max_elements } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Inspect Windows UI tree")?;
                let elements = self.windows_provider()?.subtree(
                    session_id,
                    root,
                    depth.unwrap_or(3),
                    max_elements.unwrap_or(128),
                ).map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_uia","elements":elements}))
            }
            InspectRequest::UiFind {
                session_id,
                root,
                role,
                name,
                automation_id,
                exact_name,
                depth,
                max_results,
            } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Find Windows UI elements")?;
                let elements = self.windows_provider()?.find(
                    session_id,
                    root,
                    UiFindQuery { role, name, automation_id, exact_name },
                    depth.unwrap_or(6),
                    max_results.unwrap_or(25),
                ).map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_uia","elements":elements}))
            }
            InspectRequest::UiRef { session_id, element_ref } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Inspect Windows UI element")?;
                let element = self.windows_provider()?.inspect(session_id, element_ref).map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_uia","element":element}))
            }
            InspectRequest::Applications { session_id, limit } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiInspection, "Inspect running applications")?;
                let apps = self.windows_provider()?.applications(session_id, limit.unwrap_or(64)).map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_native","applications":apps}))
            }
            InspectRequest::Audio { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::NativeSystemControl, "Read Windows volume")?;
                let audio = self.windows_provider()?.audio_state().map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_audio","audio":audio}))
            }
            InspectRequest::Clipboard { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ClipboardRead, "Read Windows clipboard")?;
                let text = self.windows_provider()?.clipboard_read().map_err(map_windows_error)?;
                to_value(json!({"route_used":"windows_clipboard","text":text}))
            }
        }
    }

    fn agent_files(
        &self,
        config: &LocalConfig,
        request: FilesRequest,
    ) -> Result<Value, ProtocolError> {
        match request {
            FilesRequest::Roots => {
                self.authorize(config, None, Capability::FilesRead, "List approved folders")?;
                to_value(RootsResponse {
                    roots: config.roots.iter().map(|root| RootResponse {
                        root_id: root.root_id,
                        display_name: root.display_name.clone(),
                    }).collect(),
                })
            }
            FilesRequest::OpenWorkspace { root_id, relative_path } => {
                self.authorize(config, None, Capability::FilesRead, "Open approved workspace")?;
                let response = self.open_approved_workspace(config, root_id, relative_path.as_deref())?;
                to_value(response)
            }
            FilesRequest::List { workspace_id, path } => {
                self.authorize(config, None, Capability::FilesRead, "List workspace files")?;
                validate_remote_path(&path)?;
                let context = self.workspace(config, workspace_id)?;
                let entries = context.fs.list_dir(&path).map_err(map_fs_error)?.into_iter().map(|entry| {
                    DirectoryEntryResponse { name: entry.name, kind: map_entry_kind(&entry.kind) }
                }).collect::<Vec<_>>();
                to_value(DirectoryResponse { entries })
            }
            FilesRequest::Stat { workspace_id, path } => {
                self.authorize(config, None, Capability::FilesRead, "Inspect workspace file")?;
                validate_remote_path(&path)?;
                let metadata = self.workspace(config, workspace_id)?.fs.metadata(&path).map_err(map_fs_error)?;
                to_value(FileStatResponse {
                    kind: map_entry_kind(&metadata.kind),
                    size_bytes: metadata.len,
                    modified_ms: metadata.modified_ms,
                })
            }
            FilesRequest::Read { workspace_id, path, max_bytes } => {
                self.authorize(config, None, Capability::FilesRead, "Read workspace file")?;
                validate_remote_path(&path)?;
                let read = self.workspace(config, workspace_id)?.fs.read_text_bounded(
                    &path,
                    max_bytes.unwrap_or(DEFAULT_READ_BYTES).clamp(1, MAX_READ_BYTES),
                ).map_err(map_fs_error)?;
                to_value(FileContentResponse { contents: read.contents, truncated: read.truncated })
            }
            FilesRequest::Write { workspace_id, path, contents, overwrite, verification } => {
                self.authorize(config, None, Capability::FilesWrite, &format!("Write {path}"))?;
                validate_payload(contents.len(), MAX_WRITE_BYTES, "file write")?;
                validate_remote_path(&path)?;
                let context = self.workspace(config, workspace_id)?;
                context.fs.write_text_with_mode(&path, &contents, overwrite).map_err(map_fs_error)?;
                let verified = if verification == VerificationMode::None {
                    None
                } else {
                    Some(context.fs.read_text(&path).map_err(map_fs_error)? == contents)
                };
                self.verified_value("filesystem", verification, verified, vec![path.clone()], json!({"path":path}))
            }
            FilesRequest::Patch { workspace_id, path, replacements, verification } => {
                self.authorize(config, None, Capability::FilesWrite, &format!("Patch {path}"))?;
                validate_remote_path(&path)?;
                if replacements.is_empty() || replacements.len() > 100 {
                    return Err(protocol_error(ErrorCode::InvalidRequest, "patch requires between 1 and 100 replacements"));
                }
                let bytes = replacements.iter().map(|replacement| replacement.old.len() + replacement.new.len()).sum::<usize>();
                validate_payload(bytes, MAX_PATCH_BYTES, "file patch")?;
                let mapped = replacements.iter().map(|replacement| Replacement {
                    old: replacement.old.clone(),
                    new: replacement.new.clone(),
                }).collect::<Vec<_>>();
                let context = self.workspace(config, workspace_id)?;
                context.fs.apply_replacements(&path, &mapped).map_err(map_fs_error)?;
                let verified = if verification == VerificationMode::None {
                    None
                } else {
                    let after = context.fs.read_text(&path).map_err(map_fs_error)?;
                    Some(replacements.iter().all(|replacement| {
                        !after.contains(&replacement.old) && after.contains(&replacement.new)
                    }))
                };
                self.verified_value("filesystem", verification, verified, vec![path.clone()], json!({"path":path}))
            }
            FilesRequest::Search { workspace_id, query, filename, content, glob, max_results } => {
                self.authorize(config, None, Capability::FilesRead, "Search workspace files")?;
                if query.is_empty() || query.chars().count() > 1024 || (!filename && !content) {
                    return Err(protocol_error(ErrorCode::InvalidRequest, "invalid file search"));
                }
                let result = self.workspace(config, workspace_id)?.fs.search(&SearchOptions {
                    query,
                    filename,
                    content,
                    glob,
                    max_results: max_results.unwrap_or(100).clamp(1, MAX_SEARCH_RESULTS),
                }).map_err(map_fs_error)?;
                to_value(SearchResponse {
                    matches: result.matches.into_iter().map(|found| SearchMatchResponse {
                        path: found.path,
                        line: found.line,
                        preview: found.preview,
                        kind: match found.kind {
                            SearchMatchKind::Filename => SearchMatchKindResponse::Filename,
                            SearchMatchKind::Content => SearchMatchKindResponse::Content,
                        },
                    }).collect(),
                    truncated: result.truncated,
                    files_scanned: result.files_scanned,
                })
            }
            FilesRequest::Mkdir { workspace_id, path } => {
                self.authorize(config, None, Capability::FilesWrite, &format!("Create folder {path}"))?;
                validate_remote_path(&path)?;
                self.workspace(config, workspace_id)?.fs.create_dir(&path).map_err(map_fs_error)?;
                to_value(json!({"route_used":"filesystem","created":path}))
            }
            FilesRequest::Move { workspace_id, from, to, overwrite } => {
                self.authorize(config, None, Capability::FilesWrite, &format!("Move {from} to {to}"))?;
                validate_remote_path(&from)?;
                validate_remote_path(&to)?;
                self.workspace(config, workspace_id)?.fs.move_path(&from, &to, overwrite).map_err(map_fs_error)?;
                self.verified_value("filesystem", VerificationMode::Auto, Some(true), vec![from, to.clone()], json!({"path":to}))
            }
            FilesRequest::Delete { workspace_id, path } => {
                self.authorize(config, None, Capability::FilesWrite, &format!("Delete {path}"))?;
                validate_remote_path(&path)?;
                let context = self.workspace(config, workspace_id)?;
                context.fs.delete_file(&path).map_err(map_fs_error)?;
                let verified = !context.fs.exists(&path).unwrap_or(true);
                self.verified_value("filesystem", VerificationMode::Auto, Some(verified), vec![path.clone()], json!({"path":path}))
            }
        }
    }

    fn agent_exec(
        &self,
        config: &LocalConfig,
        request: ExecDomainRequest,
    ) -> Result<Value, ProtocolError> {
        match request {
            ExecDomainRequest::Run { session_id, workspace_id, program, args } => {
                self.require_session_workspace(session_id, workspace_id)?;
                self.authorize(config, Some(session_id), Capability::Exec, &format!("Run {program}"))?;
                validate_command_values(&program, &args)?;
                let context = self.workspace(config, workspace_id)?;
                let result = run_blocking(&context.workspace, &CommandSpec::new(program.clone(), args)).map_err(map_exec_error)?;
                to_value(json!({
                    "route_used":"exec",
                    "exit_code":result.exit_code,
                    "stdout":result.stdout,
                    "stderr":result.stderr,
                    "duration_ms":duration_ms(result.duration),
                    "timed_out":result.timed_out,
                    "stdout_truncated":result.stdout_truncated,
                    "stderr_truncated":result.stderr_truncated
                }))
            }
            ExecDomainRequest::Start { session_id, workspace_id, program, args } => {
                self.require_session_workspace(session_id, workspace_id)?;
                self.authorize(config, Some(session_id), Capability::Exec, &format!("Start {program}"))?;
                validate_command_values(&program, &args)?;
                let context = self.workspace(config, workspace_id)?;
                let started = self.processes.start_detailed(&context.workspace, &CommandSpec::new(program.clone(), args)).map_err(map_exec_error)?;
                self.sessions.bind_process(session_id, started.process_id).map_err(map_session_error)?;
                self.events.publish(session_id, "process", "process.started", &format!("Started {program}"), Some(json!({"job_id":started.process_id,"pid":started.pid})));
                to_value(json!({"route_used":"process","job_id":started.process_id,"pid":started.pid,"started_at_ms":started.started_at_ms}))
            }
            ExecDomainRequest::Poll { session_id, job_id } => {
                self.require_owned_process(session_id, job_id)?;
                self.authorize(config, Some(session_id), Capability::Exec, "Inspect running process")?;
                let poll = self.processes.poll(job_id).map_err(map_exec_error)?;
                if !poll.stdout.text.is_empty() || !poll.stderr.text.is_empty() {
                    self.events.publish(session_id, "process", "process.output", "Process produced output", Some(json!({"job_id":job_id})));
                }
                if !matches!(poll.status.state, ProcessState::Running) {
                    self.events.publish(session_id, "process", "process.exited", "Process exited", Some(json!({"job_id":job_id,"state":map_process_state(&poll.status.state)})));
                }
                to_value(ProcessPollResponse {
                    state: map_process_state(&poll.status.state),
                    duration_ms: duration_ms(poll.status.duration),
                    stdout: map_stream_output(poll.stdout),
                    stderr: map_stream_output(poll.stderr),
                })
            }
            ExecDomainRequest::Stdin { session_id, job_id, text, close_stdin } => {
                self.require_owned_process(session_id, job_id)?;
                self.authorize(config, Some(session_id), Capability::Exec, "Send process input")?;
                self.processes.write_stdin(job_id, &text, close_stdin).map_err(map_exec_error)?;
                to_value(json!({"route_used":"process","accepted":true}))
            }
            ExecDomainRequest::Kill { session_id, job_id } => {
                self.require_owned_process(session_id, job_id)?;
                self.authorize(config, Some(session_id), Capability::Exec, "Stop running process")?;
                let status = self.processes.kill(job_id).map_err(map_exec_error)?;
                self.events.publish(session_id, "process", "process.exited", "Process stopped", Some(json!({"job_id":job_id})));
                to_value(json!({"route_used":"process","state":map_process_state(&status.state)}))
            }
            ExecDomainRequest::TerminalProfiles { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Inspect terminal profiles")?;
                to_value(json!({"route_used":"conpty","profiles":self.terminals.profiles()}))
            }
            ExecDomainRequest::TerminalCreate { session_id, workspace_id, profile_id, rows, cols } => {
                self.require_session_workspace(session_id, workspace_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Create persistent terminal")?;
                let context = self.workspace(config, workspace_id)?;
                let terminal = self.terminals.create(profile_id.as_deref(), context.workspace.root(), rows.unwrap_or(30), cols.unwrap_or(120)).map_err(map_terminal_error)?;
                self.sessions.bind_terminal(session_id, terminal.terminal_id).map_err(map_session_error)?;
                self.events.publish(session_id, "terminal", "terminal.created", "Persistent terminal created", Some(json!({"terminal_id":terminal.terminal_id,"pid":terminal.pid})));
                to_value(json!({"route_used":"conpty","terminal":terminal}))
            }
            ExecDomainRequest::TerminalWrite { session_id, terminal_id, text } => {
                self.require_owned_terminal(session_id, terminal_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Write persistent terminal")?;
                self.terminals.write(terminal_id, &text).map_err(map_terminal_error)?;
                to_value(json!({"route_used":"conpty","accepted":true}))
            }
            ExecDomainRequest::TerminalRead { session_id, terminal_id, after_sequence, max_bytes } => {
                self.require_owned_terminal(session_id, terminal_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Read persistent terminal")?;
                let snapshot = self.terminals.snapshot(terminal_id, after_sequence, max_bytes).map_err(map_terminal_error)?;
                if !snapshot.output.is_empty() {
                    self.events.publish(session_id, "terminal", "terminal.output", "Terminal produced output", Some(json!({"terminal_id":terminal_id,"sequence":snapshot.sequence})));
                }
                if !matches!(snapshot.state, TerminalState::Running) {
                    self.events.publish(session_id, "terminal", "process.exited", "Terminal process exited", Some(json!({"terminal_id":terminal_id})));
                }
                to_value(json!({"route_used":"conpty","snapshot":snapshot}))
            }
            ExecDomainRequest::TerminalResize { session_id, terminal_id, rows, cols } => {
                self.require_owned_terminal(session_id, terminal_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Resize persistent terminal")?;
                self.terminals.resize(terminal_id, rows, cols).map_err(map_terminal_error)?;
                to_value(json!({"route_used":"conpty","resized":true}))
            }
            ExecDomainRequest::TerminalInterrupt { session_id, terminal_id } => {
                self.require_owned_terminal(session_id, terminal_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Interrupt persistent terminal")?;
                self.terminals.interrupt(terminal_id).map_err(map_terminal_error)?;
                to_value(json!({"route_used":"conpty","interrupted":true}))
            }
            ExecDomainRequest::TerminalKill { session_id, terminal_id } => {
                self.require_owned_terminal(session_id, terminal_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "Kill persistent terminal")?;
                let state = self.terminals.kill(terminal_id).map_err(map_terminal_error)?;
                self.events.publish(session_id, "terminal", "process.exited", "Terminal killed", Some(json!({"terminal_id":terminal_id})));
                to_value(json!({"route_used":"conpty","state":state}))
            }
            ExecDomainRequest::TerminalList { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::Terminal, "List persistent terminals")?;
                let terminals = self.terminals.list().map_err(map_terminal_error)?.into_iter().filter(|terminal| self.sessions.owns_terminal(session_id, terminal.terminal_id)).collect::<Vec<_>>();
                to_value(json!({"route_used":"conpty","terminals":terminals}))
            }
        }
    }

    fn agent_act(&self, config: &LocalConfig, request: ActRequest) -> Result<Value, ProtocolError> {
        match request {
            ActRequest::AppLaunch { session_id, program, args, workspace_id, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ApplicationControl, &format!("Launch {program}"))?;
                let cwd = match workspace_id {
                    Some(id) => {
                        self.require_session_workspace(session_id, id)?;
                        Some(self.workspace(config, id)?.workspace.root().to_path_buf())
                    }
                    None => None,
                };
                let launched = self.windows_provider()?.launch_application(&program, &args, cwd.as_deref()).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else {
                    Some(self.wait_for_app(session_id, launched.pid, true))
                };
                self.events.publish(session_id, "windows", "window.opened", &format!("Launched {program}"), Some(json!({"pid":launched.pid})));
                self.verified_value("windows_app", verification, verified, vec![launched.pid.to_string()], json!({"launch":launched}))
            }
            ActRequest::AppActivate { session_id, pid, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ApplicationControl, "Activate application")?;
                self.windows_provider()?.activate_application(pid).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else {
                    Some(self.windows_provider()?.active_window(session_id).map(|window| window.process_id == pid).unwrap_or(false))
                };
                self.verified_value("windows_app", verification, verified, vec![pid.to_string()], json!({"pid":pid}))
            }
            ActRequest::AppQuit { session_id, pid, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ApplicationControl, "Close application")?;
                self.windows_provider()?.quit_application(pid).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else { Some(self.wait_for_app(session_id, pid, false)) };
                self.events.publish(session_id, "windows", "window.closed", "Application closed", Some(json!({"pid":pid})));
                self.verified_value("windows_app", verification, verified, vec![pid.to_string()], json!({"pid":pid}))
            }
            ActRequest::OpenTarget { session_id, target } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ApplicationControl, "Open file or URI")?;
                self.windows_provider()?.open_target(&target).map_err(map_windows_error)?;
                self.verified_value("windows_shell", VerificationMode::None, None, Vec::new(), json!({"target":target}))
            }
            ActRequest::ClipboardWrite { session_id, text, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ClipboardWrite, "Write Windows clipboard")?;
                self.windows_provider()?.clipboard_write(&text).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else {
                    Some(self.windows_provider()?.clipboard_read().map_err(map_windows_error)? == text)
                };
                self.verified_value("windows_clipboard", verification, verified, Vec::new(), json!({"characters":text.chars().count()}))
            }
            ActRequest::AudioSet { session_id, volume_percent, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::NativeSystemControl, &format!("Set Windows volume to {volume_percent}%"))?;
                let audio = self.windows_provider()?.audio_set(volume_percent).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else { Some(audio.volume_percent.abs_diff(volume_percent) <= 1) };
                self.verified_value("windows_audio", verification, verified, Vec::new(), json!({"audio":audio}))
            }
            ActRequest::Ui { session_id, element_ref, action, verification } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::UiControl, "Control Windows UI element")?;
                let result = self.windows_provider()?.act(session_id, element_ref, map_ui_action(action)).map_err(map_windows_error)?;
                let verified = if verification == VerificationMode::None { None } else { result.deterministic_verification };
                self.verified_value("windows_uia", verification, verified, vec![element_ref.to_string()], json!({"element":result.element}))
            }
            ActRequest::Screenshot { session_id, display_id, format } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::ScreenCapture, "Capture screen")?;
                let format = parse_screenshot_format(format.as_deref())?;
                let screenshot = lock(&self.computer).screenshot(display_id.as_deref(), format).map_err(map_computer_error)?;
                to_value(json!({"route_used":"raw_computer","screenshot":{
                    "display_id":screenshot.display_id,"width":screenshot.width,"height":screenshot.height,"mime_type":screenshot.mime_type,"data_base64":screenshot.data_base64
                }}))
            }
            ActRequest::RawInput { session_id, input } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::RawInput, "Send raw keyboard or mouse input")?;
                let computer = lock(&self.computer);
                match input {
                    RawInputRequest::MouseMove { x, y } => computer.mouse_move(x, y),
                    RawInputRequest::MouseClick { button } => computer.mouse_click(parse_mouse_button(&button)?),
                    RawInputRequest::MouseDrag { from_x, from_y, to_x, to_y, button } => computer.mouse_drag(from_x, from_y, to_x, to_y, parse_mouse_button(&button)?),
                    RawInputRequest::Scroll { amount, axis } => computer.scroll(amount, parse_scroll_axis(&axis)?),
                    RawInputRequest::Key { key, modifiers } => computer.key(&key, &modifiers),
                    RawInputRequest::Type { text } => computer.type_text(&text),
                }.map_err(map_computer_error)?;
                self.verified_value("raw_computer", VerificationMode::None, None, Vec::new(), json!({"accepted":true}))
            }
        }
    }

    fn agent_browser(&self, config: &LocalConfig, request: BrowserRequest) -> Result<Value, ProtocolError> {
        match request {
            BrowserRequest::Status { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Inspect browser runtime")?;
                to_value(json!({"route_used":"playwright","status":self.browser.status().map_err(map_browser_error)?}))
            }
            BrowserRequest::CreateContext { session_id, authenticated, persistent } => {
                self.require_session(session_id)?;
                let capability = if authenticated { Capability::BrowserAuthenticated } else { Capability::BrowserIsolated };
                self.authorize(config, Some(session_id), capability, if authenticated { "Create authenticated browser context" } else { "Create isolated browser context" })?;
                if authenticated {
                    return Err(protocol_error(ErrorCode::BrowserUnavailable, "authenticated existing-browser control requires the explicit Playwright extension integration; architecture is reserved but extension setup is not bundled in this beta"));
                }
                let profile = if persistent { BrowserProfile::Persistent } else { BrowserProfile::Isolated };
                let context = self.browser.create_context(profile).map_err(map_browser_error)?;
                self.sessions.bind_context(session_id, context.context_id).map_err(map_session_error)?;
                to_value(json!({"route_used":"playwright","context":context}))
            }
            BrowserRequest::ListContexts { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "List browser contexts")?;
                let contexts = self.browser.list_contexts().map_err(map_browser_error)?.into_iter().filter(|context| self.sessions.owns_context(session_id, context.context_id)).collect::<Vec<_>>();
                to_value(json!({"route_used":"playwright","contexts":contexts}))
            }
            BrowserRequest::CloseContext { session_id, context_id } => {
                self.require_owned_context(session_id, context_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Close browser context")?;
                self.browser.close_context(context_id).map_err(map_browser_error)?;
                to_value(json!({"route_used":"playwright","closed":true}))
            }
            BrowserRequest::NewTab { session_id, context_id, url } => {
                self.require_owned_context(session_id, context_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Open browser tab")?;
                let tab = self.browser.new_tab(context_id, url.as_deref()).map_err(map_browser_error)?;
                self.sessions.bind_tab(session_id, tab.tab_id).map_err(map_session_error)?;
                self.events.publish(session_id, "browser", "browser.navigation", "Browser tab opened", Some(json!({"tab_id":tab.tab_id,"url":tab.url})));
                to_value(json!({"route_used":"playwright","tab":tab}))
            }
            BrowserRequest::ListTabs { session_id, context_id } => {
                self.require_session(session_id)?;
                if let Some(context_id) = context_id { self.require_owned_context(session_id, context_id)?; }
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "List browser tabs")?;
                let tabs = self.browser.list_tabs(context_id).map_err(map_browser_error)?.into_iter().filter(|tab| self.sessions.owns_tab(session_id, tab.tab_id)).collect::<Vec<_>>();
                to_value(json!({"route_used":"playwright","tabs":tabs}))
            }
            BrowserRequest::CloseTab { session_id, tab_id } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Close browser tab")?;
                self.browser.close_tab(tab_id).map_err(map_browser_error)?;
                to_value(json!({"route_used":"playwright","closed":true}))
            }
            BrowserRequest::Navigate { session_id, tab_id, url } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Navigate browser tab")?;
                let tab = self.browser.navigate(tab_id, &url).map_err(map_browser_error)?;
                self.events.publish(session_id, "browser", "browser.navigation", "Browser navigated", Some(json!({"tab_id":tab_id,"url":tab.url})));
                to_value(json!({"route_used":"playwright","tab":tab}))
            }
            BrowserRequest::Snapshot { session_id, tab_id } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Inspect browser semantics")?;
                to_value(json!({"route_used":"playwright","snapshot":self.browser.snapshot(tab_id).map_err(map_browser_error)?}))
            }
            BrowserRequest::Find { session_id, tab_id, target, max_results } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Find browser element")?;
                let target: BrowserTarget = serde_json::from_value(target).map_err(|error| protocol_error(ErrorCode::InvalidRequest, error.to_string()))?;
                to_value(json!({"route_used":"playwright","elements":self.browser.find(tab_id, target, max_results.unwrap_or(10)).map_err(map_browser_error)?}))
            }
            BrowserRequest::Act { session_id, tab_id, target, action, browser_verification, verification } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Control browser page")?;
                let target: BrowserTarget = serde_json::from_value(target).map_err(|error| protocol_error(ErrorCode::InvalidRequest, error.to_string()))?;
                let action: BrowserAction = serde_json::from_value(action).map_err(|error| protocol_error(ErrorCode::InvalidRequest, error.to_string()))?;
                let browser_verification: Option<BrowserVerification> = browser_verification.map(serde_json::from_value).transpose().map_err(|error| protocol_error(ErrorCode::InvalidRequest, error.to_string()))?;
                let result = self.browser.act(tab_id, target, action, browser_verification).map_err(map_browser_error)?;
                let verified = if verification == VerificationMode::None { None } else { result.verification };
                self.verified_value("playwright", verification, verified, vec![tab_id.to_string()], json!({"page":result}))
            }
            BrowserRequest::Console { session_id, tab_id, after_sequence, max_entries } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Read browser console")?;
                let entries = self.browser.console(tab_id, after_sequence, max_entries.unwrap_or(50)).map_err(map_browser_error)?;
                if !entries.is_empty() { self.events.publish(session_id, "browser", "browser.console", "Browser console activity", Some(json!({"tab_id":tab_id,"count":entries.len()}))); }
                to_value(json!({"route_used":"playwright","entries":entries}))
            }
            BrowserRequest::Network { session_id, tab_id, after_sequence, max_entries } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Read browser network activity")?;
                let entries = self.browser.network(tab_id, after_sequence, max_entries.unwrap_or(50)).map_err(map_browser_error)?;
                if entries.iter().any(|entry| entry.kind == "request_failed") { self.events.publish(session_id, "browser", "browser.request_failed", "Browser request failed", Some(json!({"tab_id":tab_id}))); }
                to_value(json!({"route_used":"playwright","entries":entries}))
            }
            BrowserRequest::Downloads { session_id, tab_id, after_sequence, max_entries } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Read browser downloads")?;
                let entries = self.browser.downloads(tab_id, after_sequence, max_entries.unwrap_or(50)).map_err(map_browser_error)?;
                if !entries.is_empty() { self.events.publish(session_id, "browser", "download.completed", "Browser download completed", Some(json!({"tab_id":tab_id,"count":entries.len()}))); }
                to_value(json!({"route_used":"playwright","entries":entries}))
            }
            BrowserRequest::Screenshot { session_id, tab_id } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Capture browser screenshot")?;
                to_value(json!({"route_used":"playwright","screenshot":self.browser.screenshot(tab_id).map_err(map_browser_error)?}))
            }
            BrowserRequest::PageState { session_id, tab_id } => {
                self.require_owned_tab(session_id, tab_id)?;
                self.authorize(config, Some(session_id), Capability::BrowserIsolated, "Inspect browser page state")?;
                to_value(json!({"route_used":"playwright","page":self.browser.page_state(tab_id).map_err(map_browser_error)?}))
            }
        }
    }

    fn agent_tools(&self, config: &LocalConfig, request: ToolsRequest) -> Result<Value, ProtocolError> {
        match request {
            ToolsRequest::Providers { session_id } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::McpDiscovery, "Inspect local MCP providers")?;
                to_value(json!({"route_used":"mcp_federation","providers":self.mcp.providers(&config.mcp_servers).map_err(map_mcp_error)?}))
            }
            ToolsRequest::Search { session_id, query, provider_id, max_results } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::McpDiscovery, "Search local MCP tools")?;
                let tools = self.mcp.search(&config.mcp_servers, &query, provider_id, max_results.unwrap_or(5)).map_err(map_mcp_error)?;
                to_value(json!({"route_used":"mcp_federation","tools":tools}))
            }
            ToolsRequest::Describe { session_id, tool_ref } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::McpDiscovery, "Describe local MCP tool")?;
                let tool = self.mcp.describe(&config.mcp_servers, tool_ref).map_err(map_mcp_error)?;
                to_value(json!({"route_used":"mcp_federation","tool":tool}))
            }
            ToolsRequest::Call { session_id, tool_ref, arguments } => {
                self.require_session(session_id)?;
                self.authorize(config, Some(session_id), Capability::McpExecution, "Execute local MCP tool")?;
                let result = self.mcp.call(&config.mcp_servers, tool_ref, arguments).map_err(map_mcp_error)?;
                self.events.publish(session_id, "mcp", "mcp.called", "Local MCP tool completed", Some(json!({"tool_ref":tool_ref})));
                to_value(json!({"route_used":"mcp_federation","result":result.result}))
            }
        }
    }

    fn agent_events(&self, request: EventsRequest) -> Result<Value, ProtocolError> {
        self.require_session(request.session_id)?;
        let events = self.events.read(
            request.session_id,
            request.after_sequence,
            &request.types,
            request.wait_ms,
            request.max_events.unwrap_or(50),
        );
        to_value(json!({"events":events,"latest_sequence":self.events.latest_sequence()}))
    }

    fn authorize(
        &self,
        config: &LocalConfig,
        session_id: Option<SessionId>,
        capability: Capability,
        summary: &str,
    ) -> Result<(), ProtocolError> {
        match config.capability_policy.get(capability) {
            PermissionMode::Allow => Ok(()),
            PermissionMode::Deny => Err(protocol_error(
                ErrorCode::PermissionDenied,
                format!("{capability:?} is denied by the local Latch permission policy"),
            )),
            PermissionMode::Ask => {
                if self.local.consume_approval_grant(session_id, capability, summary).map_err(map_local_error)? {
                    return Ok(());
                }
                let approval = self.local.queue_approval(session_id, capability, summary).map_err(map_local_error)?;
                if let Some(session_id) = session_id {
                    self.events.publish(
                        session_id,
                        "permissions",
                        "approval.required",
                        summary,
                        Some(json!({"approval_id":approval.approval_id,"capability":capability})),
                    );
                }
                Err(protocol_error(
                    ErrorCode::ApprovalRequired,
                    format!("local approval required: {} ({})", approval.approval_id, approval.summary),
                ))
            }
        }
    }

    fn require_session(&self, session_id: SessionId) -> Result<(), ProtocolError> {
        self.sessions.require_active(session_id).map_err(map_session_error)
    }

    fn require_session_workspace(&self, session_id: SessionId, workspace_id: WorkspaceId) -> Result<(), ProtocolError> {
        self.require_session(session_id)?;
        if self.sessions.owns_workspace(session_id, workspace_id) {
            Ok(())
        } else {
            Err(protocol_error(ErrorCode::PermissionDenied, "workspace is not bound to this Latch session"))
        }
    }

    fn require_owned_terminal(&self, session_id: SessionId, terminal_id: TerminalId) -> Result<(), ProtocolError> {
        self.require_session(session_id)?;
        if self.sessions.owns_terminal(session_id, terminal_id) { Ok(()) } else { Err(protocol_error(ErrorCode::TerminalNotFound, "terminal is not owned by this session")) }
    }

    fn require_owned_process(&self, session_id: SessionId, process_id: ProcessId) -> Result<(), ProtocolError> {
        self.require_session(session_id)?;
        if self.sessions.owns_process(session_id, process_id) { Ok(()) } else { Err(protocol_error(ErrorCode::ProcessNotFound, "process is not owned by this session")) }
    }

    fn require_owned_context(&self, session_id: SessionId, context_id: latch_core::BrowserContextId) -> Result<(), ProtocolError> {
        self.require_session(session_id)?;
        if self.sessions.owns_context(session_id, context_id) { Ok(()) } else { Err(protocol_error(ErrorCode::BrowserUnavailable, "browser context is not owned by this session")) }
    }

    fn require_owned_tab(&self, session_id: SessionId, tab_id: latch_core::TabId) -> Result<(), ProtocolError> {
        self.require_session(session_id)?;
        if self.sessions.owns_tab(session_id, tab_id) { Ok(()) } else { Err(protocol_error(ErrorCode::BrowserUnavailable, "browser tab is not owned by this session")) }
    }

    fn windows_provider(&self) -> Result<&WindowsManager, ProtocolError> {
        self.windows.as_ref().ok_or_else(|| protocol_error(ErrorCode::ComputerUnavailable, "Windows semantic provider is unavailable"))
    }

    fn wait_for_app(&self, session_id: SessionId, pid: u32, expected_present: bool) -> bool {
        let Some(windows) = &self.windows else { return false; };
        for _ in 0..10 {
            let present = windows.applications(session_id, 64).is_ok_and(|apps| apps.iter().any(|app| app.pid == pid));
            if present == expected_present { return true; }
            thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn verified_value(
        &self,
        route: &str,
        verification_mode: VerificationMode,
        verified: Option<bool>,
        changed_refs: Vec<String>,
        result: Value,
    ) -> Result<Value, ProtocolError> {
        let outcome = match (verification_mode, verified) {
            (VerificationMode::None, _) => ActionOutcome::AppliedUnverified,
            (_, Some(true)) => ActionOutcome::Verified,
            (VerificationMode::Required, Some(false) | None) => ActionOutcome::VerificationFailed,
            (VerificationMode::Auto, Some(false)) => ActionOutcome::VerificationFailed,
            (VerificationMode::Auto, None) => ActionOutcome::AppliedUnverified,
        };
        let revision = self.revision.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        to_value(VerifiedActionResponse {
            action_id: ActionId::new(),
            route_used: route.to_owned(),
            outcome,
            verification: verified.map(Value::Bool),
            changed_refs,
            revision,
            result,
        })
    }

    fn open_approved_workspace(
        &self,
        config: &LocalConfig,
        root_id: latch_core::RootId,
        relative_path: Option<&str>,
    ) -> Result<WorkspaceResponse, ProtocolError> {
        let root = config.roots.iter().find(|root| root.root_id == root_id).ok_or_else(|| protocol_error(ErrorCode::RootNotFound, "approved root was not found"))?;
        let relative = safe_relative_path(relative_path.unwrap_or("."))?;
        let workspace = Workspace::open(root.canonical_path.join(&relative)).map_err(|error| map_workspace_error(&error, ErrorCode::RootNotFound))?;
        if !path_within(workspace.root(), &root.canonical_path) {
            return Err(protocol_error(ErrorCode::PathEscape, "workspace resolves outside the approved root"));
        }
        let relative_display = normalized_relative(&relative);
        let workspace_id = workspace.id();
        let fs = WorkspaceFs::new(workspace.clone()).map_err(map_fs_error)?;
        write_lock(&self.workspaces).insert(workspace_id, Arc::new(WorkspaceContext {
            workspace,
            fs,
            root_id: Some(root.root_id),
            developer_raw: false,
        }));
        info!(%workspace_id, "workspace opened");
        self.record("Opened folder", &format!("{}/{}", root.display_name, relative_display));
        Ok(WorkspaceResponse {
            workspace_id,
            root_id: Some(root.root_id),
            display_name: root.display_name.clone(),
            relative_path: relative_display,
            developer_raw: false,
        })
    }

    fn open_raw_workspace_internal(&self, config: &LocalConfig, path: String) -> Result<WorkspaceResponse, ProtocolError> {
        if !config.legacy_absolute_workspaces {
            return Err(protocol_error(ErrorCode::PermissionDenied, "legacy absolute workspace mode is disabled locally"));
        }
        let workspace = Workspace::open(path).map_err(|error| map_workspace_error(&error, ErrorCode::InvalidPath))?;
        let workspace_id = workspace.id();
        let fs = WorkspaceFs::new(workspace.clone()).map_err(map_fs_error)?;
        write_lock(&self.workspaces).insert(workspace_id, Arc::new(WorkspaceContext {
            workspace,
            fs,
            root_id: None,
            developer_raw: true,
        }));
        Ok(WorkspaceResponse {
            workspace_id,
            root_id: None,
            display_name: "Developer workspace".to_owned(),
            relative_path: ".".to_owned(),
            developer_raw: true,
        })
    }

    fn workspace(&self, config: &LocalConfig, workspace_id: WorkspaceId) -> Result<Arc<WorkspaceContext>, ProtocolError> {
        let context = read_lock(&self.workspaces).get(&workspace_id).cloned().ok_or_else(|| protocol_error(ErrorCode::WorkspaceExpired, "workspace is not open or has expired"))?;
        if context.developer_raw {
            if config.legacy_absolute_workspaces { return Ok(context); }
        } else if context.root_id.is_some_and(|root_id| config.roots.iter().any(|root| root.root_id == root_id)) {
            return Ok(context);
        }
        Err(protocol_error(ErrorCode::WorkspaceExpired, "workspace access was revoked locally"))
    }

    fn record(&self, action: &str, detail: &str) {
        if let Err(error) = self.local.record_activity(action, detail) {
            warn!(%error, "could not record local activity metadata");
        }
    }

    // V0.5 compatibility surface. These preserve legacy toggles while the Router migrates to agent v3.
    fn roots_list_legacy(&self, config: &LocalConfig, _request: EmptyRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        Ok(ResponsePayload::Roots(RootsResponse { roots: config.roots.iter().map(|root| RootResponse { root_id: root.root_id, display_name: root.display_name.clone() }).collect() }))
    }

    fn open_workspace_legacy(&self, config: &LocalConfig, request: WorkspaceOpenRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        Ok(ResponsePayload::Workspace(self.open_approved_workspace(config, request.root_id, request.relative_path.as_deref())?))
    }

    fn open_raw_workspace_legacy(&self, config: &LocalConfig, request: WorkspacePathRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        Ok(ResponsePayload::Workspace(self.open_raw_workspace_internal(config, request.path)?))
    }

    fn list_dir_legacy(&self, config: &LocalConfig, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        let entries = self.workspace(config, request.workspace_id)?.fs.list_dir(&request.path).map_err(map_fs_error)?.into_iter().map(|entry| DirectoryEntryResponse { name: entry.name, kind: map_entry_kind(&entry.kind) }).collect();
        Ok(ResponsePayload::Directory(DirectoryResponse { entries }))
    }

    fn stat_file_legacy(&self, config: &LocalConfig, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        let metadata = self.workspace(config, request.workspace_id)?.fs.metadata(&request.path).map_err(map_fs_error)?;
        Ok(ResponsePayload::FileStat(FileStatResponse { kind: map_entry_kind(&metadata.kind), size_bytes: metadata.len, modified_ms: metadata.modified_ms }))
    }

    fn read_file_legacy(&self, config: &LocalConfig, request: ReadRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        let read = self.workspace(config, request.workspace_id)?.fs.read_text_bounded(&request.path, request.max_bytes.unwrap_or(DEFAULT_READ_BYTES).clamp(1, MAX_READ_BYTES)).map_err(map_fs_error)?;
        Ok(ResponsePayload::FileContent(FileContentResponse { contents: read.contents, truncated: read.truncated }))
    }

    fn write_file_legacy(&self, config: &LocalConfig, request: WriteRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_payload(request.contents.len(), MAX_WRITE_BYTES, "file write")?;
        validate_remote_path(&request.path)?;
        self.workspace(config, request.workspace_id)?.fs.write_text_with_mode(&request.path, &request.contents, request.overwrite.unwrap_or(true)).map_err(map_fs_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn patch_file_legacy(&self, config: &LocalConfig, request: PatchRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        let replacements = request.replacements.into_iter().map(|replacement| Replacement { old: replacement.old, new: replacement.new }).collect::<Vec<_>>();
        self.workspace(config, request.workspace_id)?.fs.apply_replacements(&request.path, &replacements).map_err(map_fs_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn search_files_legacy(&self, config: &LocalConfig, request: SearchRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        let filename = request.filename.unwrap_or(true);
        let content = request.content.unwrap_or(true);
        let result = self.workspace(config, request.workspace_id)?.fs.search(&SearchOptions { query: request.query, filename, content, glob: request.glob, max_results: request.max_results.unwrap_or(100).clamp(1, MAX_SEARCH_RESULTS) }).map_err(map_fs_error)?;
        Ok(ResponsePayload::Search(SearchResponse { matches: result.matches.into_iter().map(|found| SearchMatchResponse { path: found.path, line: found.line, preview: found.preview, kind: match found.kind { SearchMatchKind::Filename => SearchMatchKindResponse::Filename, SearchMatchKind::Content => SearchMatchKindResponse::Content } }).collect(), truncated: result.truncated, files_scanned: result.files_scanned }))
    }

    fn create_dir_legacy(&self, config: &LocalConfig, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        self.workspace(config, request.workspace_id)?.fs.create_dir(&request.path).map_err(map_fs_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn move_file_legacy(&self, config: &LocalConfig, request: MoveRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.from)?;
        validate_remote_path(&request.to)?;
        self.workspace(config, request.workspace_id)?.fs.move_path(&request.from, &request.to, request.overwrite.unwrap_or(false)).map_err(map_fs_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn delete_file_legacy(&self, config: &LocalConfig, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.files, "file access is disabled locally")?;
        validate_remote_path(&request.path)?;
        self.workspace(config, request.workspace_id)?.fs.delete_file(&request.path).map_err(map_fs_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn run_command_legacy(&self, config: &LocalConfig, request: ExecRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.commands, "command execution is disabled locally")?;
        validate_command_values(&request.program, &request.args)?;
        let result = run_blocking(&self.workspace(config, request.workspace_id)?.workspace, &CommandSpec::new(request.program, request.args)).map_err(map_exec_error)?;
        Ok(ResponsePayload::Exec(ExecResponse { exit_code: result.exit_code, stdout: result.stdout, stderr: result.stderr, duration_ms: duration_ms(result.duration), timed_out: result.timed_out, stdout_truncated: result.stdout_truncated, stderr_truncated: result.stderr_truncated }))
    }

    fn start_process_legacy(&self, config: &LocalConfig, request: ExecRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.commands, "command execution is disabled locally")?;
        validate_command_values(&request.program, &request.args)?;
        let started = self.processes.start_detailed(&self.workspace(config, request.workspace_id)?.workspace, &CommandSpec::new(request.program, request.args)).map_err(map_exec_error)?;
        Ok(ResponsePayload::ProcessStarted(ProcessStartedResponse { job_id: started.process_id, pid: started.pid, started_at_ms: started.started_at_ms }))
    }

    fn poll_process_legacy(&self, config: &LocalConfig, request: ProcessRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.commands, "command execution is disabled locally")?;
        let poll = self.processes.poll(request.job_id).map_err(map_exec_error)?;
        Ok(ResponsePayload::ProcessPoll(ProcessPollResponse { state: map_process_state(&poll.status.state), duration_ms: duration_ms(poll.status.duration), stdout: map_stream_output(poll.stdout), stderr: map_stream_output(poll.stderr) }))
    }

    fn process_stdin_legacy(&self, config: &LocalConfig, request: ProcessStdinRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.commands, "command execution is disabled locally")?;
        self.processes.write_stdin(request.job_id, &request.text, request.close_stdin).map_err(map_exec_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn kill_process_legacy(&self, config: &LocalConfig, request: ProcessRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.commands, "command execution is disabled locally")?;
        self.processes.kill(request.job_id).map_err(map_exec_error)?;
        self.poll_process_legacy(config, request)
    }

    fn displays_legacy(&self, config: &LocalConfig, _request: EmptyRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.screen, "screen access is disabled locally")?;
        let displays = lock(&self.computer).displays().map_err(map_computer_error)?.into_iter().map(|display| DisplayResponse { display_id: display.display_id, name: display.name, x: display.x, y: display.y, width: display.width, height: display.height, scale_factor: display.scale_factor, primary: display.primary }).collect();
        Ok(ResponsePayload::Displays(DisplaysResponse { displays }))
    }

    fn screenshot_legacy(&self, config: &LocalConfig, request: ScreenshotRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.screen, "screen access is disabled locally")?;
        let format = match request.format.unwrap_or(ScreenshotFormatRequest::Jpeg) { ScreenshotFormatRequest::Jpeg => ScreenshotFormat::Jpeg, ScreenshotFormatRequest::Webp => ScreenshotFormat::WebP, ScreenshotFormatRequest::Png => ScreenshotFormat::Png };
        let screenshot = lock(&self.computer).screenshot(request.display_id.as_deref(), format).map_err(map_computer_error)?;
        Ok(ResponsePayload::Screenshot(ScreenshotResponse { display_id: screenshot.display_id, width: screenshot.width, height: screenshot.height, mime_type: screenshot.mime_type, data_base64: screenshot.data_base64 }))
    }

    fn windows_legacy(&self, config: &LocalConfig, _request: EmptyRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.screen, "screen access is disabled locally")?;
        let windows = lock(&self.computer).windows().map_err(map_computer_error)?.into_iter().map(|window| WindowResponse { window_id: window.window_id, title: window.title, process_name: window.process_name, x: window.x, y: window.y, width: window.width, height: window.height, focused: window.focused, minimized: window.minimized }).collect();
        Ok(ResponsePayload::Windows(WindowsResponse { windows }))
    }

    fn focus_window_legacy(&self, config: &LocalConfig, request: WindowRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).focus(&request.window_id).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn mouse_move_legacy(&self, config: &LocalConfig, request: PointRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).mouse_move(request.x, request.y).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn mouse_click_legacy(&self, config: &LocalConfig, request: MouseClickRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).mouse_click(map_mouse_button(request.button)).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn mouse_drag_legacy(&self, config: &LocalConfig, request: MouseDragRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).mouse_drag(request.from_x, request.from_y, request.to_x, request.to_y, map_mouse_button(request.button)).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn scroll_legacy(&self, config: &LocalConfig, request: ScrollRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        let axis = match request.axis { ScrollAxisRequest::Vertical => ScrollAxis::Vertical, ScrollAxisRequest::Horizontal => ScrollAxis::Horizontal };
        lock(&self.computer).scroll(request.amount, axis).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn key_legacy(&self, config: &LocalConfig, request: KeyRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).key(&request.key, &request.modifiers).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn type_text_legacy(&self, config: &LocalConfig, request: TypeRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.computer_control, "computer control is disabled locally")?;
        lock(&self.computer).type_text(&request.text).map_err(map_computer_error)?;
        Ok(ResponsePayload::Ack)
    }

    fn mcp_servers_legacy(&self, config: &LocalConfig, _request: EmptyRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.mcp_discovery, "local MCP discovery is disabled locally")?;
        let providers = self.mcp.providers(&config.mcp_servers).map_err(map_mcp_error)?;
        Ok(ResponsePayload::McpServers(McpServersResponse { servers: providers.into_iter().map(|provider| McpServerResponse { server_id: provider.server_id, display_name: provider.display_name, status: if provider.connected { McpServerStatusResponse::Connected } else { McpServerStatusResponse::Degraded } }).collect() }))
    }

    fn mcp_tools_legacy(&self, config: &LocalConfig, request: McpServerRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.mcp_discovery, "local MCP discovery is disabled locally")?;
        let server = remote_mcp_server(config, request.server_id)?;
        let tools = self.mcp.list_tools(server).map_err(map_mcp_error)?.into_iter().map(|tool| McpToolResponse { name: tool.name, description: tool.description, input_schema: tool.input_schema }).collect();
        Ok(ResponsePayload::McpTools(McpToolsResponse { tools }))
    }

    fn mcp_call_legacy(&self, config: &LocalConfig, request: McpCallRequest) -> Result<ResponsePayload, ProtocolError> {
        require_legacy(config.permissions.mcp_execution, "local MCP execution is disabled locally")?;
        let server = remote_mcp_server(config, request.server_id)?;
        let result = self.mcp.call_tool(server, &request.tool_name, request.arguments).map_err(map_mcp_error)?;
        Ok(ResponsePayload::McpCall(McpCallResponse { result: result.result }))
    }
}

struct WorkspaceContext {
    workspace: Workspace,
    fs: WorkspaceFs,
    root_id: Option<latch_core::RootId>,
    developer_raw: bool,
}

fn require_legacy(enabled: bool, message: &str) -> Result<(), ProtocolError> {
    if enabled { Ok(()) } else { Err(protocol_error(ErrorCode::PermissionDenied, message)) }
}

fn remote_mcp_server(config: &LocalConfig, server_id: McpServerId) -> Result<&McpServerConfig, ProtocolError> {
    let server = config.mcp_servers.iter().find(|server| server.server_id == server_id).ok_or_else(|| protocol_error(ErrorCode::McpServerNotFound, "local MCP server was not found"))?;
    if !server.enabled || !server.allow_remote {
        return Err(protocol_error(ErrorCode::McpServerDisabled, "local MCP server is not allowed through ChatGPT"));
    }
    Ok(server)
}

fn map_ui_action(action: UiActionRequest) -> UiAction {
    match action {
        UiActionRequest::Invoke => UiAction::Invoke,
        UiActionRequest::SetValue { value } => UiAction::SetValue { value },
        UiActionRequest::Select => UiAction::Select,
        UiActionRequest::Toggle => UiAction::Toggle,
        UiActionRequest::Expand => UiAction::Expand,
        UiActionRequest::Collapse => UiAction::Collapse,
        UiActionRequest::Scroll => UiAction::Scroll,
        UiActionRequest::Focus => UiAction::Focus,
    }
}

fn parse_mouse_button(value: &str) -> Result<MouseButton, ProtocolError> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        _ => Err(protocol_error(ErrorCode::InvalidRequest, "mouse button must be left, right, or middle")),
    }
}

fn parse_scroll_axis(value: &str) -> Result<ScrollAxis, ProtocolError> {
    match value.to_ascii_lowercase().as_str() {
        "vertical" => Ok(ScrollAxis::Vertical),
        "horizontal" => Ok(ScrollAxis::Horizontal),
        _ => Err(protocol_error(ErrorCode::InvalidRequest, "scroll axis must be vertical or horizontal")),
    }
}

fn parse_screenshot_format(value: Option<&str>) -> Result<ScreenshotFormat, ProtocolError> {
    match value.unwrap_or("jpeg").to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => Ok(ScreenshotFormat::Jpeg),
        "webp" => Ok(ScreenshotFormat::WebP),
        "png" => Ok(ScreenshotFormat::Png),
        _ => Err(protocol_error(ErrorCode::InvalidRequest, "screenshot format must be jpeg, webp, or png")),
    }
}

fn validate_command_values(program: &str, args: &[String]) -> Result<(), ProtocolError> {
    if program.trim().is_empty() || program.len() > 4096 {
        return Err(protocol_error(ErrorCode::InvalidRequest, "invalid command program"));
    }
    if args.len() > 256 || args.iter().any(|argument| argument.len() > 8192) {
        return Err(protocol_error(ErrorCode::PayloadTooLarge, "command arguments are too large"));
    }
    Ok(())
}

fn validate_remote_path(path: &str) -> Result<(), ProtocolError> {
    if path.chars().count() > MAX_PATH_CHARS {
        return Err(protocol_error(ErrorCode::PayloadTooLarge, "path is too large"));
    }
    safe_relative_path(path).map(|_| ())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, ProtocolError> {
    let normalized = if value.is_empty() { "." } else { value };
    if looks_like_absolute_windows_path(normalized) || normalized.starts_with("//") {
        return Err(protocol_error(ErrorCode::PathEscape, "absolute paths are not allowed"));
    }
    let path = Path::new(normalized);
    if path.is_absolute() {
        return Err(protocol_error(ErrorCode::PathEscape, "absolute paths are not allowed"));
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(protocol_error(ErrorCode::PathEscape, "path escapes the approved workspace")),
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(path.to_path_buf())
}

fn looks_like_absolute_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with("\\\\") || value.starts_with("\\?") || value.starts_with("\\.") || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn path_within(candidate: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let candidate = candidate.to_string_lossy().to_ascii_lowercase();
        let root = root.to_string_lossy().to_ascii_lowercase();
        candidate == root || candidate.strip_prefix(&root).is_some_and(|suffix| suffix.starts_with(['\\', '/']))
    }
    #[cfg(not(windows))]
    {
        candidate == root || candidate.starts_with(root)
    }
}

fn normalized_relative(path: &Path) -> String {
    if path == Path::new(".") || path.as_os_str().is_empty() { ".".to_owned() } else { path.to_string_lossy().replace('\\', "/") }
}

fn validate_payload(size: usize, limit: usize, label: &str) -> Result<(), ProtocolError> {
    if size > limit { Err(protocol_error(ErrorCode::PayloadTooLarge, format!("{label} exceeds the {limit} byte limit"))) } else { Ok(()) }
}

fn map_entry_kind(kind: &EntryKind) -> EntryKindResponse {
    match kind { EntryKind::File => EntryKindResponse::File, EntryKind::Directory => EntryKindResponse::Directory, EntryKind::Symlink => EntryKindResponse::Symlink, EntryKind::Other => EntryKindResponse::Other }
}

fn map_process_state(state: &ProcessState) -> ProcessStateResponse {
    match state { ProcessState::Running => ProcessStateResponse::Running, ProcessState::Exited { exit_code } => ProcessStateResponse::Exited { exit_code: *exit_code }, ProcessState::Failed { exit_code } => ProcessStateResponse::Failed { exit_code: *exit_code }, ProcessState::Killed => ProcessStateResponse::Killed }
}

fn map_stream_output(output: ManagedStreamOutput) -> ProcessStreamOutputResponse {
    ProcessStreamOutputResponse { text: output.text, truncated: output.truncated, complete: output.complete }
}

const fn map_mouse_button(button: MouseButtonRequest) -> MouseButton {
    match button { MouseButtonRequest::Left => MouseButton::Left, MouseButtonRequest::Right => MouseButton::Right, MouseButtonRequest::Middle => MouseButton::Middle }
}

fn map_workspace_error(error: &WorkspaceError, missing_code: ErrorCode) -> ProtocolError {
    let code = match error { WorkspaceError::NotFound { .. } => missing_code, WorkspaceError::NotDirectory { .. } => ErrorCode::InvalidPath, WorkspaceError::PermissionDenied { .. } => ErrorCode::PermissionDenied, WorkspaceError::Io { .. } => ErrorCode::Io };
    protocol_error(code, error.to_string())
}

fn map_fs_error(error: FsError) -> ProtocolError {
    let code = match error { FsError::PathOutsideWorkspace { .. } => ErrorCode::PathEscape, FsError::FileNotFound { .. } => ErrorCode::FileNotFound, FsError::AlreadyExists { .. } => ErrorCode::AlreadyExists, FsError::PatchConflict { .. } => ErrorCode::PatchConflict, FsError::PermissionDenied { .. } => ErrorCode::PermissionDenied, FsError::InvalidPath { .. } => ErrorCode::InvalidPath, FsError::Io { .. } => ErrorCode::Io };
    protocol_error(code, error.to_string())
}

fn map_exec_error(error: ExecError) -> ProtocolError {
    let code = match error { ExecError::CommandNotFound { .. } => ErrorCode::CommandNotFound, ExecError::ProcessNotFound { .. } => ErrorCode::ProcessNotFound, ExecError::ProcessFailed { .. } => ErrorCode::ProcessFailed, ExecError::PermissionDenied { .. } => ErrorCode::PermissionDenied, ExecError::Io { .. } => ErrorCode::Io };
    protocol_error(code, error.to_string())
}

fn map_computer_error(error: ComputerError) -> ProtocolError {
    let code = match error { ComputerError::UnsupportedPlatform => ErrorCode::ComputerUnavailable, ComputerError::WindowNotFound | ComputerError::InvalidInput(_) => ErrorCode::InvalidRequest, ComputerError::ScreenshotTooLarge => ErrorCode::PayloadTooLarge, ComputerError::Operation(_) => ErrorCode::ComputerUnavailable };
    protocol_error(code, error.to_string())
}

fn map_terminal_error(error: TerminalError) -> ProtocolError {
    let code = match error { TerminalError::NotFound(_) | TerminalError::NotRunning(_) => ErrorCode::TerminalNotFound, TerminalError::PayloadTooLarge => ErrorCode::PayloadTooLarge, TerminalError::ProfileNotFound(_) | TerminalError::Create(_) => ErrorCode::InvalidRequest, TerminalError::Io(_) => ErrorCode::Io };
    protocol_error(code, error.to_string())
}

fn map_windows_error(error: WindowsError) -> ProtocolError {
    let code = match error { WindowsError::Unsupported | WindowsError::UiUnavailable(_) | WindowsError::WorkerStopped => ErrorCode::ComputerUnavailable, WindowsError::StaleRef | WindowsError::RefSessionMismatch => ErrorCode::UiRefStale, WindowsError::TargetElevated => ErrorCode::TargetElevated, WindowsError::SecureDesktop => ErrorCode::SecureDesktop, WindowsError::InvalidInput(_) => ErrorCode::InvalidRequest, WindowsError::Application(_) | WindowsError::Clipboard(_) | WindowsError::Audio(_) => ErrorCode::ComputerUnavailable };
    protocol_error(code, error.to_string())
}

fn map_browser_error(error: BrowserError) -> ProtocolError {
    let code = match error { BrowserError::InvalidInput(_) => ErrorCode::InvalidRequest, BrowserError::Unavailable(_) | BrowserError::Protocol(_) | BrowserError::Operation(_) | BrowserError::Io(_) => ErrorCode::BrowserUnavailable };
    protocol_error(code, error.to_string())
}

fn map_mcp_error(error: McpClientError) -> ProtocolError {
    let code = match error { McpClientError::Timeout => ErrorCode::McpTimeout, McpClientError::ServerDisabled => ErrorCode::McpServerDisabled, McpClientError::ToolNotFound(_) => ErrorCode::McpToolNotFound, McpClientError::ToolRefNotFound => ErrorCode::McpToolRefNotFound, McpClientError::Protocol(_) => ErrorCode::McpUnavailable, McpClientError::Connect(_) | McpClientError::MissingEnvironmentReference(_) | McpClientError::Runtime(_) | McpClientError::ManagerStopped => ErrorCode::McpUnavailable };
    protocol_error(code, error.to_string())
}

fn map_local_error(error: LocalError) -> ProtocolError {
    protocol_error(ErrorCode::Io, error.to_string())
}

fn map_session_error(error: String) -> ProtocolError {
    let code = if error.contains("closed") { ErrorCode::SessionClosed } else { ErrorCode::SessionNotFound };
    protocol_error(code, error)
}

fn protocol_error(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
    ProtocolError { code, message: message.into() }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn to_value(value: impl Serialize) -> Result<Value, ProtocolError> {
    serde_json::to_value(value).map_err(|error| protocol_error(ErrorCode::Io, error.to_string()))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use latch_local::{PermissionPolicy, PermissionPreset};

    fn engine_with_root() -> (tempfile::TempDir, Engine, latch_core::RootId) {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let projects = temp.path().join("projects");
        std::fs::create_dir(&projects).unwrap();
        let store = LocalStore::new(&state);
        let root = store.add_root(&projects).unwrap();
        store.set_capability_policy(PermissionPolicy::preset(PermissionPreset::Developer)).unwrap();
        (temp, Engine::with_store(store), root.root_id)
    }

    #[test]
    fn sessions_bind_workspace_and_verified_file_write() {
        let (_temp, engine, root_id) = engine_with_root();
        let session: SessionSnapshot = serde_json::from_value(match engine.handle(Request::Agent(AgentRequest::Session(SessionRequest::Create))).unwrap() { ResponsePayload::Agent(value) => value, _ => panic!("wrong payload") }).unwrap();
        let workspace: WorkspaceResponse = serde_json::from_value(match engine.handle(Request::Agent(AgentRequest::Files(FilesRequest::OpenWorkspace { root_id, relative_path: None }))).unwrap() { ResponsePayload::Agent(value) => value, _ => panic!("wrong payload") }).unwrap();
        engine.handle(Request::Agent(AgentRequest::Session(SessionRequest::Update { session_id: session.session_id, workspace_ids: vec![workspace.workspace_id] }))).unwrap();
        let response = engine.handle(Request::Agent(AgentRequest::Files(FilesRequest::Write { workspace_id: workspace.workspace_id, path: "test.txt".to_owned(), contents: "Latch".to_owned(), overwrite: true, verification: VerificationMode::Required }))).unwrap();
        let ResponsePayload::Agent(value) = response else { panic!("wrong payload") };
        assert_eq!(value["outcome"], json!("verified"));
    }

    #[test]
    fn denied_permission_does_not_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::new(temp.path());
        let mut policy = PermissionPolicy::preset(PermissionPreset::Observe);
        policy.ui_control = PermissionMode::Deny;
        store.set_capability_policy(policy).unwrap();
        let engine = Engine::with_store(store);
        let session = engine.sessions.create().unwrap();
        let error = engine.agent_act(&engine.local.load().unwrap(), ActRequest::Ui { session_id: session.session_id, element_ref: UiRef::new(), action: UiActionRequest::Invoke, verification: VerificationMode::Auto }).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn path_validation_rejects_absolute_and_parent_paths() {
        assert!(safe_relative_path("../secret").is_err());
        assert!(safe_relative_path("C:\\Windows").is_err());
        assert!(safe_relative_path("src/main.rs").is_ok());
    }
}
