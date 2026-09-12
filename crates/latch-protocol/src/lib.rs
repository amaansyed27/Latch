use latch_core::{ProcessId, WorkspaceId};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestEnvelope {
    pub id: String,
    pub version: u16,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    #[serde(rename = "workspace.open")]
    WorkspaceOpen(WorkspacePathRequest),
    #[serde(rename = "workspace.create")]
    WorkspaceCreate(WorkspacePathRequest),
    #[serde(rename = "fs.read")]
    FsRead(PathRequest),
    #[serde(rename = "fs.write")]
    FsWrite(WriteRequest),
    #[serde(rename = "fs.delete")]
    FsDelete(PathRequest),
    #[serde(rename = "fs.mkdir")]
    FsMkdir(PathRequest),
    #[serde(rename = "fs.list")]
    FsList(PathRequest),
    #[serde(rename = "exec.run")]
    ExecRun(ExecRequest),
    #[serde(rename = "exec.start")]
    ExecStart(ExecRequest),
    #[serde(rename = "exec.status")]
    ExecStatus(ProcessRequest),
    #[serde(rename = "exec.output")]
    ExecOutput(ProcessRequest),
    #[serde(rename = "exec.kill")]
    ExecKill(ProcessRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePathRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathRequest {
    pub workspace_id: WorkspaceId,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteRequest {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecRequest {
    pub workspace_id: WorkspaceId,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessRequest {
    pub process_id: ProcessId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseEnvelope {
    pub id: Option<String>,
    pub version: u16,
    #[serde(flatten)]
    pub outcome: ResponseOutcome,
}

impl ResponseEnvelope {
    pub fn success(id: String, result: ResponsePayload) -> Self {
        Self {
            id: Some(id),
            version: PROTOCOL_VERSION,
            outcome: ResponseOutcome::Ok { result },
        }
    }

    pub fn error(id: Option<String>, error: ProtocolError) -> Self {
        Self {
            id,
            version: PROTOCOL_VERSION,
            outcome: ResponseOutcome::Error { error },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseOutcome {
    Ok { result: ResponsePayload },
    Error { error: ProtocolError },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ResponsePayload {
    Workspace(WorkspaceResponse),
    FileContent(FileContentResponse),
    Directory(DirectoryResponse),
    Ack,
    Exec(ExecResponse),
    ProcessStarted(ProcessStartedResponse),
    ProcessStatus(ProcessStatusResponse),
    ProcessOutput(ProcessOutputResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceResponse {
    pub workspace_id: WorkspaceId,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileContentResponse {
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectoryResponse {
    pub entries: Vec<DirectoryEntryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectoryEntryResponse {
    pub name: String,
    pub kind: EntryKindResponse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKindResponse {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecResponse {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStartedResponse {
    pub process_id: ProcessId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStatusResponse {
    pub state: ProcessStateResponse,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProcessStateResponse {
    Running,
    Exited { exit_code: Option<i32> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStreamOutputResponse {
    pub text: String,
    pub truncated: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOutputResponse {
    pub stdout: ProcessStreamOutputResponse,
    pub stderr: ProcessStreamOutputResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    WorkspaceNotFound,
    PathOutsideWorkspace,
    FileNotFound,
    PermissionDenied,
    InvalidPath,
    CommandNotFound,
    ProcessNotFound,
    ProcessFailed,
    Io,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn request_is_versioned_and_strongly_tagged() {
        let request: RequestEnvelope = serde_json::from_str(
            r#"{"id":"1","version":1,"method":"fs.read","params":{"workspace_id":"00000000-0000-0000-0000-000000000001","path":"hello.txt"}}"#,
        )
        .unwrap();

        assert_eq!(request.version, PROTOCOL_VERSION);
        assert!(matches!(request.request, Request::FsRead(_)));
    }

    #[test]
    fn exec_response_round_trips_with_execution_state() {
        let response = ResponseEnvelope::success(
            "run-1".to_owned(),
            ResponsePayload::Exec(ExecResponse {
                exit_code: None,
                stdout: "partial".to_owned(),
                stderr: "warning".to_owned(),
                duration_ms: 250,
                timed_out: true,
                stdout_truncated: false,
                stderr_truncated: true,
            }),
        );

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "run-1",
                "version": 1,
                "status": "ok",
                "result": {
                    "type": "exec",
                    "data": {
                        "exit_code": null,
                        "stdout": "partial",
                        "stderr": "warning",
                        "duration_ms": 250,
                        "timed_out": true,
                        "stdout_truncated": false,
                        "stderr_truncated": true
                    }
                }
            })
        );

        let decoded: ResponseEnvelope = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, response);
    }
}
