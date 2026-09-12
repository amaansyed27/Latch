use std::collections::HashMap;

use latch_core::{Workspace, WorkspaceError, WorkspaceId};
use latch_exec::{run_blocking, CommandSpec, ExecError, ProcessManager, ProcessState};
use latch_fs::{EntryKind, FsError, WorkspaceFs};
use latch_protocol::{
    DirectoryEntryResponse, DirectoryResponse, EntryKindResponse, ErrorCode, ExecResponse,
    FileContentResponse, ProcessOutputResponse, ProcessStartedResponse, ProcessStateResponse,
    ProcessStatusResponse, ProtocolError, Request, ResponsePayload, WorkspaceResponse,
};
use tracing::info;

pub struct Engine {
    workspaces: HashMap<WorkspaceId, WorkspaceContext>,
    processes: ProcessManager,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            workspaces: HashMap::new(),
            processes: ProcessManager::new(),
        }
    }

    pub fn handle(&mut self, request: Request) -> Result<ResponsePayload, ProtocolError> {
        match request {
            Request::WorkspaceOpen(request) => {
                let workspace = Workspace::open(request.path).map_err(map_workspace_error)?;
                self.register_workspace(workspace)
            }
            Request::WorkspaceCreate(request) => {
                let workspace = Workspace::create(request.path).map_err(map_workspace_error)?;
                self.register_workspace(workspace)
            }
            Request::FsRead(request) => {
                let context = self.workspace(request.workspace_id)?;
                let contents = context.fs.read_text(request.path).map_err(map_fs_error)?;
                Ok(ResponsePayload::FileContent(FileContentResponse { contents }))
            }
            Request::FsWrite(request) => {
                let context = self.workspace(request.workspace_id)?;
                context
                    .fs
                    .write_text(request.path, &request.contents)
                    .map_err(map_fs_error)?;
                Ok(ResponsePayload::Ack)
            }
            Request::FsDelete(request) => {
                let context = self.workspace(request.workspace_id)?;
                context.fs.delete_file(request.path).map_err(map_fs_error)?;
                Ok(ResponsePayload::Ack)
            }
            Request::FsMkdir(request) => {
                let context = self.workspace(request.workspace_id)?;
                context.fs.create_dir(request.path).map_err(map_fs_error)?;
                Ok(ResponsePayload::Ack)
            }
            Request::FsList(request) => {
                let context = self.workspace(request.workspace_id)?;
                let entries = context
                    .fs
                    .list_dir(request.path)
                    .map_err(map_fs_error)?
                    .into_iter()
                    .map(|entry| DirectoryEntryResponse {
                        name: entry.name,
                        kind: map_entry_kind(entry.kind),
                    })
                    .collect();
                Ok(ResponsePayload::Directory(DirectoryResponse { entries }))
            }
            Request::ExecRun(request) => {
                let context = self.workspace(request.workspace_id)?;
                let spec = CommandSpec::new(request.program, request.args);
                let result = run_blocking(&context.workspace, &spec).map_err(map_exec_error)?;
                Ok(ResponsePayload::Exec(ExecResponse {
                    exit_code: result.exit_code,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    duration_ms: duration_ms(result.duration),
                }))
            }
            Request::ExecStart(request) => {
                let context = self.workspace(request.workspace_id)?;
                let spec = CommandSpec::new(request.program, request.args);
                let process_id = self
                    .processes
                    .start(&context.workspace, &spec)
                    .map_err(map_exec_error)?;
                Ok(ResponsePayload::ProcessStarted(ProcessStartedResponse {
                    process_id,
                }))
            }
            Request::ExecStatus(request) => {
                let status = self
                    .processes
                    .status(request.process_id)
                    .map_err(map_exec_error)?;
                Ok(ResponsePayload::ProcessStatus(ProcessStatusResponse {
                    state: map_process_state(status.state),
                    duration_ms: duration_ms(status.duration),
                }))
            }
            Request::ExecOutput(request) => {
                let output = self
                    .processes
                    .output(request.process_id)
                    .map_err(map_exec_error)?;
                Ok(ResponsePayload::ProcessOutput(ProcessOutputResponse {
                    stdout: output.stdout,
                    stderr: output.stderr,
                    stdout_truncated: output.stdout_truncated,
                    stderr_truncated: output.stderr_truncated,
                    stdout_complete: output.stdout_complete,
                    stderr_complete: output.stderr_complete,
                }))
            }
            Request::ExecKill(request) => {
                let status = self
                    .processes
                    .kill(request.process_id)
                    .map_err(map_exec_error)?;
                Ok(ResponsePayload::ProcessStatus(ProcessStatusResponse {
                    state: map_process_state(status.state),
                    duration_ms: duration_ms(status.duration),
                }))
            }
        }
    }

    fn register_workspace(&mut self, workspace: Workspace) -> Result<ResponsePayload, ProtocolError> {
        let root = workspace.root().to_string_lossy().into_owned();
        let workspace_id = workspace.id();
        let fs = WorkspaceFs::new(workspace.clone()).map_err(map_fs_error)?;
        self.workspaces
            .insert(workspace_id, WorkspaceContext { workspace, fs });
        info!(%workspace_id, root = %root, "workspace opened");

        Ok(ResponsePayload::Workspace(WorkspaceResponse {
            workspace_id,
            root,
        }))
    }

    fn workspace(&self, workspace_id: WorkspaceId) -> Result<&WorkspaceContext, ProtocolError> {
        self.workspaces.get(&workspace_id).ok_or_else(|| ProtocolError {
            code: ErrorCode::WorkspaceNotFound,
            message: format!("workspace {workspace_id} is not open"),
        })
    }
}

struct WorkspaceContext {
    workspace: Workspace,
    fs: WorkspaceFs,
}

fn map_entry_kind(kind: EntryKind) -> EntryKindResponse {
    match kind {
        EntryKind::File => EntryKindResponse::File,
        EntryKind::Directory => EntryKindResponse::Directory,
        EntryKind::Symlink => EntryKindResponse::Symlink,
        EntryKind::Other => EntryKindResponse::Other,
    }
}

fn map_process_state(state: ProcessState) -> ProcessStateResponse {
    match state {
        ProcessState::Running => ProcessStateResponse::Running,
        ProcessState::Exited { exit_code } => ProcessStateResponse::Exited { exit_code },
    }
}

fn map_workspace_error(error: WorkspaceError) -> ProtocolError {
    let code = match &error {
        WorkspaceError::NotFound { .. } => ErrorCode::WorkspaceNotFound,
        WorkspaceError::NotDirectory { .. } => ErrorCode::InvalidPath,
        WorkspaceError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        WorkspaceError::Io { .. } => ErrorCode::Io,
    };
    ProtocolError {
        code,
        message: error.to_string(),
    }
}

fn map_fs_error(error: FsError) -> ProtocolError {
    let code = match &error {
        FsError::PathOutsideWorkspace { .. } => ErrorCode::PathOutsideWorkspace,
        FsError::FileNotFound { .. } => ErrorCode::FileNotFound,
        FsError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        FsError::InvalidPath { .. } => ErrorCode::InvalidPath,
        FsError::Io { .. } => ErrorCode::Io,
    };
    ProtocolError {
        code,
        message: error.to_string(),
    }
}

fn map_exec_error(error: ExecError) -> ProtocolError {
    let code = match &error {
        ExecError::CommandNotFound { .. } => ErrorCode::CommandNotFound,
        ExecError::ProcessNotFound { .. } => ErrorCode::ProcessNotFound,
        ExecError::ProcessFailed { .. } => ErrorCode::ProcessFailed,
        ExecError::Io { .. } => ErrorCode::Io,
    };
    ProtocolError {
        code,
        message: error.to_string(),
    }
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
