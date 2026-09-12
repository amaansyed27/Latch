use std::collections::HashMap;

use latch_core::{Workspace, WorkspaceError, WorkspaceId};
use latch_exec::{
    run_blocking, CommandSpec, ExecError, ManagedStreamOutput, ProcessManager, ProcessState,
};
use latch_fs::{EntryKind, FsError, WorkspaceFs};
use latch_protocol::{
    DirectoryEntryResponse, DirectoryResponse, EntryKindResponse, ErrorCode, ExecRequest,
    ExecResponse, FileContentResponse, PathRequest, ProcessOutputResponse, ProcessRequest,
    ProcessStartedResponse, ProcessStateResponse, ProcessStatusResponse,
    ProcessStreamOutputResponse, ProtocolError, Request, ResponsePayload, WorkspaceResponse,
    WriteRequest,
};
use tracing::info;

#[derive(Default)]
pub struct Engine {
    workspaces: HashMap<WorkspaceId, WorkspaceContext>,
    processes: ProcessManager,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, request: Request) -> Result<ResponsePayload, ProtocolError> {
        match request {
            Request::WorkspaceOpen(request) => self.open_workspace(request.path),
            Request::WorkspaceCreate(request) => self.create_workspace(request.path),
            Request::FsRead(request) => self.read_file(request),
            Request::FsWrite(request) => self.write_file(request),
            Request::FsDelete(request) => self.delete_file(request),
            Request::FsMkdir(request) => self.create_dir(request),
            Request::FsList(request) => self.list_dir(request),
            Request::ExecRun(request) => self.run_command(request),
            Request::ExecStart(request) => self.start_process(request),
            Request::ExecStatus(request) => self.process_status(request),
            Request::ExecOutput(request) => self.process_output(request),
            Request::ExecKill(request) => self.kill_process(request),
        }
    }

    fn open_workspace(&mut self, path: String) -> Result<ResponsePayload, ProtocolError> {
        let workspace = Workspace::open(path).map_err(|error| map_workspace_error(&error))?;
        self.register_workspace(workspace)
    }

    fn create_workspace(&mut self, path: String) -> Result<ResponsePayload, ProtocolError> {
        let workspace = Workspace::create(path).map_err(|error| map_workspace_error(&error))?;
        self.register_workspace(workspace)
    }

    fn read_file(&self, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        let contents = context
            .fs
            .read_text(request.path)
            .map_err(|error| map_fs_error(&error))?;
        Ok(ResponsePayload::FileContent(FileContentResponse {
            contents,
        }))
    }

    fn write_file(&self, request: WriteRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        context
            .fs
            .write_text(request.path, &request.contents)
            .map_err(|error| map_fs_error(&error))?;
        Ok(ResponsePayload::Ack)
    }

    fn delete_file(&self, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        context
            .fs
            .delete_file(request.path)
            .map_err(|error| map_fs_error(&error))?;
        Ok(ResponsePayload::Ack)
    }

    fn create_dir(&self, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        context
            .fs
            .create_dir(request.path)
            .map_err(|error| map_fs_error(&error))?;
        Ok(ResponsePayload::Ack)
    }

    fn list_dir(&self, request: PathRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        let entries = context
            .fs
            .list_dir(request.path)
            .map_err(|error| map_fs_error(&error))?
            .into_iter()
            .map(|entry| DirectoryEntryResponse {
                name: entry.name,
                kind: map_entry_kind(&entry.kind),
            })
            .collect();
        Ok(ResponsePayload::Directory(DirectoryResponse { entries }))
    }

    fn run_command(&self, request: ExecRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        let spec = CommandSpec::new(request.program, request.args);
        let result =
            run_blocking(&context.workspace, &spec).map_err(|error| map_exec_error(&error))?;
        Ok(ResponsePayload::Exec(ExecResponse {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            duration_ms: duration_ms(result.duration),
        }))
    }

    fn start_process(&self, request: ExecRequest) -> Result<ResponsePayload, ProtocolError> {
        let context = self.workspace(request.workspace_id)?;
        let spec = CommandSpec::new(request.program, request.args);
        let process_id = self
            .processes
            .start(&context.workspace, &spec)
            .map_err(|error| map_exec_error(&error))?;
        Ok(ResponsePayload::ProcessStarted(ProcessStartedResponse {
            process_id,
        }))
    }

    fn process_status(&self, request: ProcessRequest) -> Result<ResponsePayload, ProtocolError> {
        let status = self
            .processes
            .status(request.process_id)
            .map_err(|error| map_exec_error(&error))?;
        Ok(ResponsePayload::ProcessStatus(ProcessStatusResponse {
            state: map_process_state(&status.state),
            duration_ms: duration_ms(status.duration),
        }))
    }

    fn process_output(&self, request: ProcessRequest) -> Result<ResponsePayload, ProtocolError> {
        let output = self
            .processes
            .output(request.process_id)
            .map_err(|error| map_exec_error(&error))?;
        Ok(ResponsePayload::ProcessOutput(ProcessOutputResponse {
            stdout: map_stream_output(output.stdout),
            stderr: map_stream_output(output.stderr),
        }))
    }

    fn kill_process(&self, request: ProcessRequest) -> Result<ResponsePayload, ProtocolError> {
        let status = self
            .processes
            .kill(request.process_id)
            .map_err(|error| map_exec_error(&error))?;
        Ok(ResponsePayload::ProcessStatus(ProcessStatusResponse {
            state: map_process_state(&status.state),
            duration_ms: duration_ms(status.duration),
        }))
    }

    fn register_workspace(
        &mut self,
        workspace: Workspace,
    ) -> Result<ResponsePayload, ProtocolError> {
        let root = workspace.root().to_string_lossy().into_owned();
        let workspace_id = workspace.id();
        let fs = WorkspaceFs::new(workspace.clone()).map_err(|error| map_fs_error(&error))?;
        self.workspaces
            .insert(workspace_id, WorkspaceContext { workspace, fs });
        info!(%workspace_id, root = %root, "workspace opened");

        Ok(ResponsePayload::Workspace(WorkspaceResponse {
            workspace_id,
            root,
        }))
    }

    fn workspace(&self, workspace_id: WorkspaceId) -> Result<&WorkspaceContext, ProtocolError> {
        self.workspaces
            .get(&workspace_id)
            .ok_or_else(|| ProtocolError {
                code: ErrorCode::WorkspaceNotFound,
                message: format!("workspace {workspace_id} is not open"),
            })
    }
}

struct WorkspaceContext {
    workspace: Workspace,
    fs: WorkspaceFs,
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
    }
}

fn map_stream_output(output: ManagedStreamOutput) -> ProcessStreamOutputResponse {
    ProcessStreamOutputResponse {
        text: output.text,
        truncated: output.truncated,
        complete: output.complete,
    }
}

fn map_workspace_error(error: &WorkspaceError) -> ProtocolError {
    let code = match error {
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

fn map_fs_error(error: &FsError) -> ProtocolError {
    let code = match error {
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

fn map_exec_error(error: &ExecError) -> ProtocolError {
    let code = match error {
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
