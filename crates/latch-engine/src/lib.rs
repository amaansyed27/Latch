use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

use latch_computer::{ComputerError, ComputerManager, MouseButton, ScreenshotFormat, ScrollAxis};
use latch_core::{McpServerId, Workspace, WorkspaceError, WorkspaceId};
use latch_exec::{
    run_blocking, CommandSpec, ExecError, ManagedStreamOutput, ProcessManager, ProcessState,
};
use latch_fs::{EntryKind, FsError, Replacement, SearchMatchKind, SearchOptions, WorkspaceFs};
use latch_local::{LocalConfig, LocalError, LocalStore, McpServerConfig};
use latch_mcp_client::{self, McpClientError};
use latch_protocol::{
    DirectoryEntryResponse, DirectoryResponse, DisplayResponse, DisplaysResponse, EmptyRequest,
    EntryKindResponse, ErrorCode, ExecRequest, ExecResponse, FileContentResponse, FileStatResponse,
    KeyRequest, McpCallRequest, McpCallResponse, McpServerRequest, McpServerResponse,
    McpServerStatusResponse, McpServersResponse, McpToolResponse, McpToolsResponse,
    MouseButtonRequest, MouseClickRequest, MouseDragRequest, MoveRequest, PatchRequest,
    PathRequest, PointRequest, ProcessPollResponse, ProcessRequest, ProcessStartedResponse,
    ProcessStateResponse, ProcessStdinRequest, ProcessStreamOutputResponse, ProtocolError,
    ReadRequest, Request, RequestEnvelope, ResponseEnvelope, ResponsePayload, RootResponse,
    RootsResponse, ScreenshotFormatRequest, ScreenshotRequest, ScreenshotResponse,
    ScrollAxisRequest, ScrollRequest, SearchMatchKindResponse, SearchMatchResponse, SearchRequest,
    SearchResponse, TypeRequest, WindowRequest, WindowResponse, WindowsResponse,
    WorkspaceOpenRequest, WorkspacePathRequest, WorkspaceResponse, WriteRequest, PROTOCOL_VERSION,
};
use tracing::{error, info, warn};

const DEFAULT_READ_BYTES: usize = 1024 * 1024;
const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const MAX_WRITE_BYTES: usize = 1024 * 1024;
const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_PATH_CHARS: usize = 4096;

pub struct Engine {
    workspaces: HashMap<WorkspaceId, WorkspaceContext>,
    processes: ProcessManager,
    computer: ComputerManager,
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
        Self {
            workspaces: HashMap::new(),
            processes: ProcessManager::new(),
            computer: ComputerManager::new(),
            local,
        }
    }

    pub const fn local_store(&self) -> &LocalStore {
        &self.local
    }

    pub fn handle_envelope(&mut self, request: RequestEnvelope) -> ResponseEnvelope {
        if request.version != PROTOCOL_VERSION {
            return ResponseEnvelope::error(
                Some(request.id),
                ProtocolError {
                    code: ErrorCode::UnsupportedVersion,
                    message: format!(
                        "protocol version {} is unsupported; expected {PROTOCOL_VERSION}",
                        request.version
                    ),
                },
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

    pub fn handle(&mut self, request: Request) -> Result<ResponsePayload, ProtocolError> {
        let config = self.local.load().map_err(map_local_error)?;
        if config.paused {
            return Err(protocol_error(
                ErrorCode::RemotePaused,
                "remote access is paused on this computer",
            ));
        }
        match request {
            Request::RootsList(request) => self.roots_list(&config, request),
            Request::WorkspaceOpen(request) => self.open_workspace(&config, request),
            Request::WorkspaceOpenRaw(request) => self.open_raw_workspace(&config, request),
            Request::FsList(request) => self.list_dir(&config, request),
            Request::FsStat(request) => self.stat_file(&config, request),
            Request::FsRead(request) => self.read_file(&config, request),
            Request::FsWrite(request) => self.write_file(&config, request),
            Request::FsPatch(request) => self.patch_file(&config, request),
            Request::FsSearch(request) => self.search_files(&config, request),
            Request::FsMkdir(request) => self.create_dir(&config, request),
            Request::FsMove(request) => self.move_file(&config, request),
            Request::FsDelete(request) => self.delete_file(&config, request),
            Request::ExecRun(request) => self.run_command(&config, request),
            Request::ExecStart(request) => self.start_process(&config, request),
            Request::ExecPoll(request) => self.poll_process(&config, request),
            Request::ExecStdin(request) => self.process_stdin(&config, request),
            Request::ExecKill(request) => self.kill_process(&config, request),
            Request::ComputerDisplays(request) => self.displays(&config, request),
            Request::ComputerScreenshot(request) => self.screenshot(&config, request),
            Request::ComputerWindows(request) => self.windows(&config, request),
            Request::ComputerFocus(request) => self.focus_window(&config, request),
            Request::ComputerMouseMove(request) => self.mouse_move(&config, request),
            Request::ComputerMouseClick(request) => self.mouse_click(&config, request),
            Request::ComputerMouseDrag(request) => self.mouse_drag(&config, request),
            Request::ComputerScroll(request) => self.scroll(&config, request),
            Request::ComputerKey(request) => self.key(&config, request),
            Request::ComputerType(request) => self.type_text(&config, request),
            Request::McpServers(request) => self.mcp_servers(&config, request),
            Request::McpTools(request) => self.mcp_tools(&config, request),
            Request::McpCall(request) => self.mcp_call(&config, request),
        }
    }

    pub fn shutdown(&self) {
        self.processes.shutdown_all();
    }

    fn roots_list(
        &self,
        config: &LocalConfig,
        _request: EmptyRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        Ok(ResponsePayload::Roots(RootsResponse {
            roots: config
                .roots
                .iter()
                .map(|root| RootResponse {
                    root_id: root.root_id,
                    display_name: root.display_name.clone(),
                })
                .collect(),
        }))
    }

    fn open_workspace(
        &mut self,
        config: &LocalConfig,
        request: WorkspaceOpenRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        let root = config
            .roots
            .iter()
            .find(|root| root.root_id == request.root_id)
            .ok_or_else(|| {
                protocol_error(ErrorCode::RootNotFound, "approved root was not found")
            })?;
        let relative_text = request.relative_path.as_deref().unwrap_or(".");
        let relative = safe_relative_path(relative_text)?;
        let workspace = Workspace::open(root.canonical_path.join(&relative))
            .map_err(|error| map_workspace_error(&error, ErrorCode::RootNotFound))?;
        if !path_within(workspace.root(), &root.canonical_path) {
            return Err(protocol_error(
                ErrorCode::PathEscape,
                "workspace resolves outside the approved root",
            ));
        }
        let relative_display = normalized_relative(&relative);
        let result = self.register_workspace(
            workspace,
            Some(root.root_id),
            root.display_name.clone(),
            relative_display.clone(),
            false,
        )?;
        self.record(
            "Opened folder",
            &format!("{}/{}", root.display_name, relative_display),
        );
        Ok(result)
    }

    fn open_raw_workspace(
        &mut self,
        config: &LocalConfig,
        request: WorkspacePathRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        if !config.legacy_absolute_workspaces {
            return Err(protocol_error(
                ErrorCode::PermissionDenied,
                "legacy absolute workspace mode is disabled locally",
            ));
        }
        require_files(config)?;
        let workspace = Workspace::open(request.path)
            .map_err(|error| map_workspace_error(&error, ErrorCode::InvalidPath))?;
        self.register_workspace(
            workspace,
            None,
            "Developer workspace".to_owned(),
            ".".to_owned(),
            true,
        )
    }

    fn list_dir(
        &self,
        config: &LocalConfig,
        request: PathRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        let context = self.workspace(config, request.workspace_id)?;
        validate_remote_path(&request.path)?;
        let entries = context
            .fs
            .list_dir(&request.path)
            .map_err(map_fs_error)?
            .into_iter()
            .map(|entry| DirectoryEntryResponse {
                name: entry.name,
                kind: map_entry_kind(&entry.kind),
            })
            .collect();
        self.record("Listed files", &request.path);
        Ok(ResponsePayload::Directory(DirectoryResponse { entries }))
    }

    fn stat_file(
        &self,
        config: &LocalConfig,
        request: PathRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        let context = self.workspace(config, request.workspace_id)?;
        validate_remote_path(&request.path)?;
        let metadata = context.fs.metadata(&request.path).map_err(map_fs_error)?;
        self.record("Inspected file", &request.path);
        Ok(ResponsePayload::FileStat(FileStatResponse {
            kind: map_entry_kind(&metadata.kind),
            size_bytes: metadata.len,
            modified_ms: metadata.modified_ms,
        }))
    }

    fn read_file(
        &self,
        config: &LocalConfig,
        request: ReadRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        let context = self.workspace(config, request.workspace_id)?;
        validate_remote_path(&request.path)?;
        let max_bytes = request
            .max_bytes
            .unwrap_or(DEFAULT_READ_BYTES)
            .clamp(1, MAX_READ_BYTES);
        let read = context
            .fs
            .read_text_bounded(&request.path, max_bytes)
            .map_err(map_fs_error)?;
        self.record("Read file", &request.path);
        Ok(ResponsePayload::FileContent(FileContentResponse {
            contents: read.contents,
            truncated: read.truncated,
        }))
    }

    fn write_file(
        &self,
        config: &LocalConfig,
        request: WriteRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        validate_payload(request.contents.len(), MAX_WRITE_BYTES, "file write")?;
        validate_remote_path(&request.path)?;
        let context = self.workspace(config, request.workspace_id)?;
        context
            .fs
            .write_text_with_mode(
                &request.path,
                &request.contents,
                request.overwrite.unwrap_or(true),
            )
            .map_err(map_fs_error)?;
        self.record("Wrote file", &request.path);
        Ok(ResponsePayload::Ack)
    }

    fn patch_file(
        &self,
        config: &LocalConfig,
        request: PatchRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        validate_remote_path(&request.path)?;
        if request.replacements.is_empty() || request.replacements.len() > 100 {
            return Err(protocol_error(
                ErrorCode::InvalidRequest,
                "patch requires between 1 and 100 replacements",
            ));
        }
        let patch_bytes = request
            .replacements
            .iter()
            .map(|replacement| replacement.old.len() + replacement.new.len())
            .sum::<usize>();
        validate_payload(patch_bytes, MAX_PATCH_BYTES, "file patch")?;
        let replacements = request
            .replacements
            .into_iter()
            .map(|replacement| Replacement {
                old: replacement.old,
                new: replacement.new,
            })
            .collect::<Vec<_>>();
        let context = self.workspace(config, request.workspace_id)?;
        context
            .fs
            .apply_replacements(&request.path, &replacements)
            .map_err(map_fs_error)?;
        self.record("Patched file", &request.path);
        Ok(ResponsePayload::Ack)
    }

    fn search_files(
        &self,
        config: &LocalConfig,
        request: SearchRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        if request.query.is_empty() || request.query.chars().count() > 1024 {
            return Err(protocol_error(
                ErrorCode::InvalidRequest,
                "search query must contain 1 to 1024 characters",
            ));
        }
        let filename = request.filename.unwrap_or(true);
        let content = request.content.unwrap_or(true);
        if !filename && !content {
            return Err(protocol_error(
                ErrorCode::InvalidRequest,
                "search must enable filename, content, or both",
            ));
        }
        if request
            .glob
            .as_ref()
            .is_some_and(|glob| glob.len() > MAX_PATH_CHARS)
        {
            return Err(protocol_error(
                ErrorCode::InvalidRequest,
                "search glob is too large",
            ));
        }
        let context = self.workspace(config, request.workspace_id)?;
        let result = context
            .fs
            .search(&SearchOptions {
                query: request.query.clone(),
                filename,
                content,
                glob: request.glob,
                max_results: request
                    .max_results
                    .unwrap_or(100)
                    .clamp(1, MAX_SEARCH_RESULTS),
            })
            .map_err(map_fs_error)?;
        self.record("Searched files", &request.query);
        Ok(ResponsePayload::Search(SearchResponse {
            matches: result
                .matches
                .into_iter()
                .map(|found| SearchMatchResponse {
                    path: found.path,
                    line: found.line,
                    preview: found.preview,
                    kind: match found.kind {
                        SearchMatchKind::Filename => SearchMatchKindResponse::Filename,
                        SearchMatchKind::Content => SearchMatchKindResponse::Content,
                    },
                })
                .collect(),
            truncated: result.truncated,
            files_scanned: result.files_scanned,
        }))
    }

    fn create_dir(
        &self,
        config: &LocalConfig,
        request: PathRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        validate_remote_path(&request.path)?;
        let context = self.workspace(config, request.workspace_id)?;
        context.fs.create_dir(&request.path).map_err(map_fs_error)?;
        self.record("Created directory", &request.path);
        Ok(ResponsePayload::Ack)
    }

    fn move_file(
        &self,
        config: &LocalConfig,
        request: MoveRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        validate_remote_path(&request.from)?;
        validate_remote_path(&request.to)?;
        let context = self.workspace(config, request.workspace_id)?;
        context
            .fs
            .move_path(
                &request.from,
                &request.to,
                request.overwrite.unwrap_or(false),
            )
            .map_err(map_fs_error)?;
        self.record("Moved file", &format!("{} -> {}", request.from, request.to));
        Ok(ResponsePayload::Ack)
    }

    fn delete_file(
        &self,
        config: &LocalConfig,
        request: PathRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_files(config)?;
        validate_remote_path(&request.path)?;
        let context = self.workspace(config, request.workspace_id)?;
        context
            .fs
            .delete_file(&request.path)
            .map_err(map_fs_error)?;
        self.record("Deleted file", &request.path);
        Ok(ResponsePayload::Ack)
    }

    fn run_command(
        &self,
        config: &LocalConfig,
        request: ExecRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_commands(config)?;
        validate_command(&request)?;
        let context = self.workspace(config, request.workspace_id)?;
        let program = request.program.clone();
        let spec = CommandSpec::new(request.program, request.args);
        let result = run_blocking(&context.workspace, &spec).map_err(map_exec_error)?;
        self.record("Ran command", &program);
        Ok(ResponsePayload::Exec(ExecResponse {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            duration_ms: duration_ms(result.duration),
            timed_out: result.timed_out,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
        }))
    }

    fn start_process(
        &self,
        config: &LocalConfig,
        request: ExecRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_commands(config)?;
        validate_command(&request)?;
        let context = self.workspace(config, request.workspace_id)?;
        let program = request.program.clone();
        let spec = CommandSpec::new(request.program, request.args);
        let started = self
            .processes
            .start_detailed(&context.workspace, &spec)
            .map_err(map_exec_error)?;
        self.record("Started command", &program);
        Ok(ResponsePayload::ProcessStarted(ProcessStartedResponse {
            job_id: started.process_id,
            pid: started.pid,
            started_at_ms: started.started_at_ms,
        }))
    }

    fn poll_process(
        &self,
        config: &LocalConfig,
        request: ProcessRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_commands(config)?;
        let poll = self
            .processes
            .poll(request.job_id)
            .map_err(map_exec_error)?;
        Ok(ResponsePayload::ProcessPoll(ProcessPollResponse {
            state: map_process_state(&poll.status.state),
            duration_ms: duration_ms(poll.status.duration),
            stdout: map_stream_output(poll.stdout),
            stderr: map_stream_output(poll.stderr),
        }))
    }

    fn process_stdin(
        &self,
        config: &LocalConfig,
        request: ProcessStdinRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_commands(config)?;
        self.processes
            .write_stdin(request.job_id, &request.text, request.close_stdin)
            .map_err(map_exec_error)?;
        self.record("Sent command input", &request.job_id.to_string());
        Ok(ResponsePayload::Ack)
    }

    fn kill_process(
        &self,
        config: &LocalConfig,
        request: ProcessRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_commands(config)?;
        self.processes
            .kill(request.job_id)
            .map_err(map_exec_error)?;
        let poll = self
            .processes
            .poll(request.job_id)
            .map_err(map_exec_error)?;
        self.record("Stopped command", &request.job_id.to_string());
        Ok(ResponsePayload::ProcessPoll(ProcessPollResponse {
            state: map_process_state(&poll.status.state),
            duration_ms: duration_ms(poll.status.duration),
            stdout: map_stream_output(poll.stdout),
            stderr: map_stream_output(poll.stderr),
        }))
    }

    fn displays(
        &self,
        config: &LocalConfig,
        _request: EmptyRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_screen(config)?;
        let displays = self
            .computer
            .displays()
            .map_err(map_computer_error)?
            .into_iter()
            .map(|display| DisplayResponse {
                display_id: display.display_id,
                name: display.name,
                x: display.x,
                y: display.y,
                width: display.width,
                height: display.height,
                scale_factor: display.scale_factor,
                primary: display.primary,
            })
            .collect();
        Ok(ResponsePayload::Displays(DisplaysResponse { displays }))
    }

    fn screenshot(
        &self,
        config: &LocalConfig,
        request: ScreenshotRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_screen(config)?;
        let format = match request.format.unwrap_or(ScreenshotFormatRequest::Jpeg) {
            ScreenshotFormatRequest::Jpeg => ScreenshotFormat::Jpeg,
            ScreenshotFormatRequest::Webp => ScreenshotFormat::WebP,
            ScreenshotFormatRequest::Png => ScreenshotFormat::Png,
        };
        let screenshot = self
            .computer
            .screenshot(request.display_id.as_deref(), format)
            .map_err(map_computer_error)?;
        self.record("Captured screen", &screenshot.display_id);
        Ok(ResponsePayload::Screenshot(ScreenshotResponse {
            display_id: screenshot.display_id,
            width: screenshot.width,
            height: screenshot.height,
            mime_type: screenshot.mime_type,
            data_base64: screenshot.data_base64,
        }))
    }

    fn windows(
        &mut self,
        config: &LocalConfig,
        _request: EmptyRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_screen(config)?;
        let windows = self
            .computer
            .windows()
            .map_err(map_computer_error)?
            .into_iter()
            .map(|window| WindowResponse {
                window_id: window.window_id,
                title: window.title,
                process_name: window.process_name,
                x: window.x,
                y: window.y,
                width: window.width,
                height: window.height,
                focused: window.focused,
                minimized: window.minimized,
            })
            .collect();
        self.record("Listed windows", "visible application windows");
        Ok(ResponsePayload::Windows(WindowsResponse { windows }))
    }

    fn focus_window(
        &self,
        config: &LocalConfig,
        request: WindowRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .focus(&request.window_id)
            .map_err(map_computer_error)?;
        self.record("Focused window", &request.window_id);
        Ok(ResponsePayload::Ack)
    }

    fn mouse_move(
        &self,
        config: &LocalConfig,
        request: PointRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .mouse_move(request.x, request.y)
            .map_err(map_computer_error)?;
        self.record("Moved pointer", &format!("{},{}", request.x, request.y));
        Ok(ResponsePayload::Ack)
    }

    fn mouse_click(
        &self,
        config: &LocalConfig,
        request: MouseClickRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .mouse_click(map_mouse_button(request.button))
            .map_err(map_computer_error)?;
        self.record("Clicked pointer", "mouse click");
        Ok(ResponsePayload::Ack)
    }

    fn mouse_drag(
        &self,
        config: &LocalConfig,
        request: MouseDragRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .mouse_drag(
                request.from_x,
                request.from_y,
                request.to_x,
                request.to_y,
                map_mouse_button(request.button),
            )
            .map_err(map_computer_error)?;
        self.record("Dragged pointer", "mouse drag");
        Ok(ResponsePayload::Ack)
    }

    fn scroll(
        &self,
        config: &LocalConfig,
        request: ScrollRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        let axis = match request.axis {
            ScrollAxisRequest::Vertical => ScrollAxis::Vertical,
            ScrollAxisRequest::Horizontal => ScrollAxis::Horizontal,
        };
        self.computer
            .scroll(request.amount, axis)
            .map_err(map_computer_error)?;
        self.record("Scrolled", &request.amount.to_string());
        Ok(ResponsePayload::Ack)
    }

    fn key(
        &self,
        config: &LocalConfig,
        request: KeyRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .key(&request.key, &request.modifiers)
            .map_err(map_computer_error)?;
        self.record("Pressed key", &request.key);
        Ok(ResponsePayload::Ack)
    }

    fn type_text(
        &self,
        config: &LocalConfig,
        request: TypeRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_control(config)?;
        self.computer
            .type_text(&request.text)
            .map_err(map_computer_error)?;
        self.record(
            "Typed text",
            &format!("{} characters", request.text.chars().count()),
        );
        Ok(ResponsePayload::Ack)
    }

    fn mcp_servers(
        &self,
        config: &LocalConfig,
        _request: EmptyRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_mcp_discovery(config)?;
        Ok(ResponsePayload::McpServers(McpServersResponse {
            servers: config
                .mcp_servers
                .iter()
                .filter(|server| server.enabled && server.allow_remote)
                .map(|server| McpServerResponse {
                    server_id: server.server_id,
                    display_name: server.display_name.clone(),
                    status: McpServerStatusResponse::Stopped,
                })
                .collect(),
        }))
    }

    fn mcp_tools(
        &self,
        config: &LocalConfig,
        request: McpServerRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_mcp_discovery(config)?;
        let server = remote_mcp_server(config, request.server_id)?;
        let tools = latch_mcp_client::list_tools(server)
            .map_err(map_mcp_error)?
            .into_iter()
            .map(|tool| McpToolResponse {
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect();
        self.record("Inspected local MCP", &server.display_name);
        Ok(ResponsePayload::McpTools(McpToolsResponse { tools }))
    }

    fn mcp_call(
        &self,
        config: &LocalConfig,
        request: McpCallRequest,
    ) -> Result<ResponsePayload, ProtocolError> {
        require_mcp_execution(config)?;
        let server = remote_mcp_server(config, request.server_id)?;
        let tools = latch_mcp_client::list_tools(server).map_err(map_mcp_error)?;
        if !tools.iter().any(|tool| tool.name == request.tool_name) {
            return Err(protocol_error(
                ErrorCode::McpToolNotFound,
                "local MCP tool was not found",
            ));
        }
        let result = latch_mcp_client::call_tool(server, &request.tool_name, request.arguments)
            .map_err(map_mcp_error)?;
        self.record(
            "Called local MCP",
            &format!("{} / {}", server.display_name, request.tool_name),
        );
        Ok(ResponsePayload::McpCall(McpCallResponse {
            result: result.result,
        }))
    }

    fn register_workspace(
        &mut self,
        workspace: Workspace,
        root_id: Option<latch_core::RootId>,
        display_name: String,
        relative_path: String,
        developer_raw: bool,
    ) -> Result<ResponsePayload, ProtocolError> {
        let workspace_id = workspace.id();
        let fs = WorkspaceFs::new(workspace.clone()).map_err(map_fs_error)?;
        self.workspaces.insert(
            workspace_id,
            WorkspaceContext {
                workspace,
                fs,
                root_id,
                developer_raw,
            },
        );
        info!(%workspace_id, %display_name, %relative_path, developer_raw, "workspace opened");
        Ok(ResponsePayload::Workspace(WorkspaceResponse {
            workspace_id,
            root_id,
            display_name,
            relative_path,
            developer_raw,
        }))
    }

    fn workspace(
        &self,
        config: &LocalConfig,
        workspace_id: WorkspaceId,
    ) -> Result<&WorkspaceContext, ProtocolError> {
        let context = self.workspaces.get(&workspace_id).ok_or_else(|| {
            protocol_error(
                ErrorCode::WorkspaceExpired,
                "workspace is not open or has expired",
            )
        })?;
        if context.developer_raw {
            if config.legacy_absolute_workspaces {
                return Ok(context);
            }
        } else if context
            .root_id
            .is_some_and(|root_id| config.roots.iter().any(|root| root.root_id == root_id))
        {
            return Ok(context);
        }
        Err(protocol_error(
            ErrorCode::WorkspaceExpired,
            "workspace access was revoked locally",
        ))
    }

    fn record(&self, action: &str, detail: &str) {
        if let Err(error) = self.local.record_activity(action, detail) {
            warn!(%error, "could not record local activity metadata");
        }
    }
}

struct WorkspaceContext {
    workspace: Workspace,
    fs: WorkspaceFs,
    root_id: Option<latch_core::RootId>,
    developer_raw: bool,
}

fn require_files(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.files {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::PermissionDenied,
            "file access is disabled locally",
        ))
    }
}

fn require_commands(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.commands {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::PermissionDenied,
            "command execution is disabled locally",
        ))
    }
}

fn require_screen(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.screen {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::ComputerReadDisabled,
            "screen access is disabled locally",
        ))
    }
}

fn require_control(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.computer_control {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::ComputerControlDisabled,
            "mouse and keyboard control is disabled locally",
        ))
    }
}

fn require_mcp_discovery(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.mcp_discovery {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::PermissionDenied,
            "local MCP discovery is disabled locally",
        ))
    }
}

fn require_mcp_execution(config: &LocalConfig) -> Result<(), ProtocolError> {
    if config.permissions.mcp_execution {
        Ok(())
    } else {
        Err(protocol_error(
            ErrorCode::PermissionDenied,
            "local MCP execution is disabled locally",
        ))
    }
}

fn remote_mcp_server(
    config: &LocalConfig,
    server_id: McpServerId,
) -> Result<&McpServerConfig, ProtocolError> {
    let server = config
        .mcp_servers
        .iter()
        .find(|server| server.server_id == server_id)
        .ok_or_else(|| {
            protocol_error(
                ErrorCode::McpServerNotFound,
                "local MCP server was not found",
            )
        })?;
    if !server.enabled || !server.allow_remote {
        return Err(protocol_error(
            ErrorCode::McpServerDisabled,
            "local MCP server is not allowed through ChatGPT",
        ));
    }
    Ok(server)
}

fn validate_command(request: &ExecRequest) -> Result<(), ProtocolError> {
    if request.program.trim().is_empty() || request.program.len() > 4096 {
        return Err(protocol_error(
            ErrorCode::InvalidRequest,
            "invalid command program",
        ));
    }
    if request.args.len() > 256 || request.args.iter().any(|argument| argument.len() > 8192) {
        return Err(protocol_error(
            ErrorCode::PayloadTooLarge,
            "command arguments are too large",
        ));
    }
    Ok(())
}

fn validate_remote_path(path: &str) -> Result<(), ProtocolError> {
    if path.chars().count() > MAX_PATH_CHARS {
        return Err(protocol_error(
            ErrorCode::PayloadTooLarge,
            "path is too large",
        ));
    }
    safe_relative_path(path).map(|_| ())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, ProtocolError> {
    let normalized = if value.is_empty() { "." } else { value };
    if looks_like_absolute_windows_path(normalized) || normalized.starts_with("//") {
        return Err(protocol_error(
            ErrorCode::PathEscape,
            "absolute paths are not allowed",
        ));
    }
    let path = Path::new(normalized);
    if path.is_absolute() {
        return Err(protocol_error(
            ErrorCode::PathEscape,
            "absolute paths are not allowed",
        ));
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(protocol_error(
                    ErrorCode::PathEscape,
                    "path escapes the approved workspace",
                ));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(path.to_path_buf())
}

fn looks_like_absolute_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with("\\\\")
        || value.starts_with("\\?")
        || value.starts_with("\\.")
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn path_within(candidate: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let candidate = candidate.to_string_lossy().to_ascii_lowercase();
        let root = root.to_string_lossy().to_ascii_lowercase();
        candidate == root
            || candidate
                .strip_prefix(&root)
                .is_some_and(|suffix| suffix.starts_with(['\\', '/']))
    }
    #[cfg(not(windows))]
    {
        candidate == root || candidate.starts_with(root)
    }
}

fn normalized_relative(path: &Path) -> String {
    if path == Path::new(".") || path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn validate_payload(size: usize, limit: usize, label: &str) -> Result<(), ProtocolError> {
    if size > limit {
        Err(protocol_error(
            ErrorCode::PayloadTooLarge,
            format!("{label} exceeds the {limit} byte limit"),
        ))
    } else {
        Ok(())
    }
}

fn map_entry_kind(kind: &EntryKind) -> EntryKindResponse {
    match kind {
        EntryKind::File => EntryKindResponse::File,
        EntryKind::Directory => EntryKindResponse::Directory,
        EntryKind::Symlink => EntryKindResponse::Symlink,
        EntryKind::Other => EntryKindResponse::Other,
    }
}

fn map_process_state(state: &ProcessState) -> ProcessStateResponse {
    match state {
        ProcessState::Running => ProcessStateResponse::Running,
        ProcessState::Exited { exit_code } => ProcessStateResponse::Exited {
            exit_code: *exit_code,
        },
        ProcessState::Failed { exit_code } => ProcessStateResponse::Failed {
            exit_code: *exit_code,
        },
        ProcessState::Killed => ProcessStateResponse::Killed,
    }
}

fn map_stream_output(output: ManagedStreamOutput) -> ProcessStreamOutputResponse {
    ProcessStreamOutputResponse {
        text: output.text,
        truncated: output.truncated,
        complete: output.complete,
    }
}

const fn map_mouse_button(button: MouseButtonRequest) -> MouseButton {
    match button {
        MouseButtonRequest::Left => MouseButton::Left,
        MouseButtonRequest::Right => MouseButton::Right,
        MouseButtonRequest::Middle => MouseButton::Middle,
    }
}

fn map_workspace_error(error: &WorkspaceError, missing_code: ErrorCode) -> ProtocolError {
    let code = match error {
        WorkspaceError::NotFound { .. } => missing_code,
        WorkspaceError::NotDirectory { .. } => ErrorCode::InvalidPath,
        WorkspaceError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        WorkspaceError::Io { .. } => ErrorCode::Io,
    };
    protocol_error(code, error.to_string())
}

fn map_fs_error(error: FsError) -> ProtocolError {
    let code = match error {
        FsError::PathOutsideWorkspace { .. } => ErrorCode::PathEscape,
        FsError::FileNotFound { .. } => ErrorCode::FileNotFound,
        FsError::AlreadyExists { .. } => ErrorCode::AlreadyExists,
        FsError::PatchConflict { .. } => ErrorCode::PatchConflict,
        FsError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        FsError::InvalidPath { .. } => ErrorCode::InvalidPath,
        FsError::Io { .. } => ErrorCode::Io,
    };
    protocol_error(code, error.to_string())
}

fn map_exec_error(error: ExecError) -> ProtocolError {
    let code = match error {
        ExecError::CommandNotFound { .. } => ErrorCode::CommandNotFound,
        ExecError::ProcessNotFound { .. } => ErrorCode::ProcessNotFound,
        ExecError::ProcessFailed { .. } => ErrorCode::ProcessFailed,
        ExecError::Io { .. } => ErrorCode::Io,
    };
    protocol_error(code, error.to_string())
}

fn map_computer_error(error: ComputerError) -> ProtocolError {
    let code = match error {
        ComputerError::InvalidInput(_) | ComputerError::WindowNotFound => ErrorCode::InvalidRequest,
        ComputerError::ScreenshotTooLarge => ErrorCode::PayloadTooLarge,
        ComputerError::UnsupportedPlatform | ComputerError::Operation(_) => {
            ErrorCode::ComputerUnavailable
        }
    };
    protocol_error(code, error.to_string())
}

fn map_mcp_error(error: McpClientError) -> ProtocolError {
    let code = match error {
        McpClientError::Timeout => ErrorCode::McpTimeout,
        McpClientError::Connect(_)
        | McpClientError::Protocol(_)
        | McpClientError::MissingEnvironmentReference(_)
        | McpClientError::Runtime(_) => ErrorCode::McpUnavailable,
    };
    protocol_error(code, error.to_string())
}

fn map_local_error(error: LocalError) -> ProtocolError {
    protocol_error(ErrorCode::Io, error.to_string())
}

fn protocol_error(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code,
        message: message.into(),
    }
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use latch_protocol::{ResponseOutcome, RootResponse};

    use super::*;

    fn engine_with_root() -> (tempfile::TempDir, Engine, RootResponse) {
        let temp = tempfile::tempdir().unwrap();
        let root_path = temp.path().join("projects");
        fs::create_dir(&root_path).unwrap();
        fs::create_dir(root_path.join("app")).unwrap();
        let store = LocalStore::new(temp.path().join("state"));
        let root = store.add_root(&root_path).unwrap();
        (
            temp,
            Engine::with_store(store),
            RootResponse {
                root_id: root.root_id,
                display_name: root.display_name,
            },
        )
    }

    #[test]
    fn envelope_rejects_unsupported_protocol_versions() {
        let (_temp, mut engine, root) = engine_with_root();
        let response = engine.handle_envelope(RequestEnvelope {
            id: "bad-version".to_owned(),
            version: PROTOCOL_VERSION + 1,
            request: Request::WorkspaceOpen(WorkspaceOpenRequest {
                root_id: root.root_id,
                relative_path: None,
            }),
        });
        match response.outcome {
            ResponseOutcome::Error { error } => {
                assert_eq!(error.code, ErrorCode::UnsupportedVersion)
            }
            outcome @ ResponseOutcome::Ok { .. } => panic!("unexpected response: {outcome:?}"),
        }
    }

    #[test]
    fn approved_root_open_never_returns_absolute_path() {
        let (temp, mut engine, root) = engine_with_root();
        let payload = engine
            .handle(Request::WorkspaceOpen(WorkspaceOpenRequest {
                root_id: root.root_id,
                relative_path: Some("app".to_owned()),
            }))
            .unwrap();
        let encoded = serde_json::to_string(&payload).unwrap();
        assert!(!encoded.contains(&temp.path().to_string_lossy().to_string()));
        assert!(encoded.contains("app"));
    }

    #[test]
    fn raw_absolute_workspace_is_off_by_default() {
        let (temp, mut engine, _root) = engine_with_root();
        let error = engine
            .handle(Request::WorkspaceOpenRaw(WorkspacePathRequest {
                path: temp.path().display().to_string(),
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn root_relative_escape_is_rejected() {
        let (_temp, mut engine, root) = engine_with_root();
        let error = engine
            .handle(Request::WorkspaceOpen(WorkspaceOpenRequest {
                root_id: root.root_id,
                relative_path: Some("../outside".to_owned()),
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PathEscape);
    }

    #[test]
    fn pause_wins_immediately() {
        let (_temp, mut engine, _root) = engine_with_root();
        engine.local_store().set_paused(true).unwrap();
        let error = engine
            .handle(Request::RootsList(EmptyRequest {}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RemotePaused);
    }

    #[test]
    fn screen_permission_defaults_off() {
        let (_temp, mut engine, _root) = engine_with_root();
        let error = engine
            .handle(Request::ComputerDisplays(EmptyRequest {}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ComputerReadDisabled);
    }

    #[test]
    fn windows_absolute_syntax_is_rejected_on_every_platform() {
        assert_eq!(
            safe_relative_path("C:\\Windows\\System32")
                .unwrap_err()
                .code,
            ErrorCode::PathEscape
        );
        assert_eq!(
            safe_relative_path("\\\\server\\share").unwrap_err().code,
            ErrorCode::PathEscape
        );
    }
}
