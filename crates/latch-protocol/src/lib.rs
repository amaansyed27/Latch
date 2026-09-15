use latch_core::{McpServerId, ProcessId, RootId, WorkspaceId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestEnvelope {
    pub id: String,
    pub version: u16,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    #[serde(rename = "roots.list")]
    RootsList(EmptyRequest),
    #[serde(rename = "workspace.open")]
    WorkspaceOpen(WorkspaceOpenRequest),
    #[serde(rename = "workspace.open_raw")]
    WorkspaceOpenRaw(WorkspacePathRequest),
    #[serde(rename = "fs.list")]
    FsList(PathRequest),
    #[serde(rename = "fs.stat")]
    FsStat(PathRequest),
    #[serde(rename = "fs.read")]
    FsRead(ReadRequest),
    #[serde(rename = "fs.write")]
    FsWrite(WriteRequest),
    #[serde(rename = "fs.patch")]
    FsPatch(PatchRequest),
    #[serde(rename = "fs.search")]
    FsSearch(SearchRequest),
    #[serde(rename = "fs.mkdir")]
    FsMkdir(PathRequest),
    #[serde(rename = "fs.move")]
    FsMove(MoveRequest),
    #[serde(rename = "fs.delete")]
    FsDelete(PathRequest),
    #[serde(rename = "exec.run")]
    ExecRun(ExecRequest),
    #[serde(rename = "exec.start")]
    ExecStart(ExecRequest),
    #[serde(rename = "exec.poll")]
    ExecPoll(ProcessRequest),
    #[serde(rename = "exec.stdin")]
    ExecStdin(ProcessStdinRequest),
    #[serde(rename = "exec.kill")]
    ExecKill(ProcessRequest),
    #[serde(rename = "computer.displays")]
    ComputerDisplays(EmptyRequest),
    #[serde(rename = "computer.screenshot")]
    ComputerScreenshot(ScreenshotRequest),
    #[serde(rename = "computer.windows")]
    ComputerWindows(EmptyRequest),
    #[serde(rename = "computer.focus")]
    ComputerFocus(WindowRequest),
    #[serde(rename = "computer.mouse_move")]
    ComputerMouseMove(PointRequest),
    #[serde(rename = "computer.mouse_click")]
    ComputerMouseClick(MouseClickRequest),
    #[serde(rename = "computer.mouse_drag")]
    ComputerMouseDrag(MouseDragRequest),
    #[serde(rename = "computer.scroll")]
    ComputerScroll(ScrollRequest),
    #[serde(rename = "computer.key")]
    ComputerKey(KeyRequest),
    #[serde(rename = "computer.type")]
    ComputerType(TypeRequest),
    #[serde(rename = "mcp.servers")]
    McpServers(EmptyRequest),
    #[serde(rename = "mcp.tools")]
    McpTools(McpServerRequest),
    #[serde(rename = "mcp.call")]
    McpCall(McpCallRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EmptyRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceOpenRequest {
    pub root_id: RootId,
    #[serde(default)]
    pub relative_path: Option<String>,
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
pub struct ReadRequest {
    pub workspace_id: WorkspaceId,
    pub path: String,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteRequest {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub contents: String,
    #[serde(default)]
    pub overwrite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplacementRequest {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchRequest {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub replacements: Vec<ReplacementRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRequest {
    pub workspace_id: WorkspaceId,
    pub query: String,
    #[serde(default)]
    pub filename: Option<bool>,
    #[serde(default)]
    pub content: Option<bool>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveRequest {
    pub workspace_id: WorkspaceId,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub overwrite: Option<bool>,
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
    pub job_id: ProcessId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStdinRequest {
    pub job_id: ProcessId,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub close_stdin: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotFormatRequest {
    Jpeg,
    Webp,
    Png,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenshotRequest {
    #[serde(default)]
    pub display_id: Option<String>,
    #[serde(default)]
    pub format: Option<ScreenshotFormatRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowRequest {
    pub window_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointRequest {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButtonRequest {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MouseClickRequest {
    #[serde(default = "default_mouse_button")]
    pub button: MouseButtonRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MouseDragRequest {
    pub from_x: i32,
    pub from_y: i32,
    pub to_x: i32,
    pub to_y: i32,
    #[serde(default = "default_mouse_button")]
    pub button: MouseButtonRequest,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAxisRequest {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScrollRequest {
    pub amount: i32,
    #[serde(default = "default_scroll_axis")]
    pub axis: ScrollAxisRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyRequest {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeRequest {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerRequest {
    pub server_id: McpServerId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCallRequest {
    pub server_id: McpServerId,
    pub tool_name: String,
    #[serde(default = "empty_json_object")]
    pub arguments: Value,
}

fn default_mouse_button() -> MouseButtonRequest {
    MouseButtonRequest::Left
}

fn default_scroll_axis() -> ScrollAxisRequest {
    ScrollAxisRequest::Vertical
}

fn empty_json_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseOutcome {
    Ok { result: ResponsePayload },
    Error { error: ProtocolError },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ResponsePayload {
    Roots(RootsResponse),
    Workspace(WorkspaceResponse),
    Directory(DirectoryResponse),
    FileStat(FileStatResponse),
    FileContent(FileContentResponse),
    Search(SearchResponse),
    Ack,
    Exec(ExecResponse),
    ProcessStarted(ProcessStartedResponse),
    ProcessPoll(ProcessPollResponse),
    Displays(DisplaysResponse),
    Screenshot(ScreenshotResponse),
    Windows(WindowsResponse),
    McpServers(McpServersResponse),
    McpTools(McpToolsResponse),
    McpCall(McpCallResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootResponse {
    pub root_id: RootId,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootsResponse {
    pub roots: Vec<RootResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceResponse {
    pub workspace_id: WorkspaceId,
    pub root_id: Option<RootId>,
    pub display_name: String,
    pub relative_path: String,
    pub developer_raw: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileContentResponse {
    pub contents: String,
    pub truncated: bool,
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
pub struct FileStatResponse {
    pub kind: EntryKindResponse,
    pub size_bytes: u64,
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMatchKindResponse {
    Filename,
    Content,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchMatchResponse {
    pub path: String,
    pub line: Option<usize>,
    pub preview: Option<String>,
    pub kind: SearchMatchKindResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResponse {
    pub matches: Vec<SearchMatchResponse>,
    pub truncated: bool,
    pub files_scanned: usize,
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
    pub job_id: ProcessId,
    pub pid: u32,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProcessStateResponse {
    Running,
    Exited { exit_code: Option<i32> },
    Failed { exit_code: Option<i32> },
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStreamOutputResponse {
    pub text: String,
    pub truncated: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessPollResponse {
    pub state: ProcessStateResponse,
    pub duration_ms: u64,
    pub stdout: ProcessStreamOutputResponse,
    pub stderr: ProcessStreamOutputResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayResponse {
    pub display_id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplaysResponse {
    pub displays: Vec<DisplayResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenshotResponse {
    pub display_id: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowResponse {
    pub window_id: String,
    pub title: String,
    pub process_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
    pub minimized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowsResponse {
    pub windows: Vec<WindowResponse>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerStatusResponse {
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerResponse {
    pub server_id: McpServerId,
    pub display_name: String,
    pub status: McpServerStatusResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServersResponse {
    pub servers: Vec<McpServerResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolResponse {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolsResponse {
    pub tools: Vec<McpToolResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCallResponse {
    pub result: Value,
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
    PermissionDenied,
    RootNotFound,
    WorkspaceExpired,
    PathEscape,
    FileNotFound,
    AlreadyExists,
    PatchConflict,
    InvalidPath,
    PayloadTooLarge,
    CommandNotFound,
    ProcessNotFound,
    ProcessFailed,
    ComputerReadDisabled,
    ComputerControlDisabled,
    ComputerUnavailable,
    McpServerNotFound,
    McpServerDisabled,
    McpToolNotFound,
    McpTimeout,
    McpUnavailable,
    RemotePaused,
    DeviceOffline,
    Io,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn approved_workspace_request_is_versioned_and_has_no_os_path() {
        let request: RequestEnvelope = serde_json::from_str(
            r#"{"id":"1","version":2,"method":"workspace.open","params":{"root_id":"00000000-0000-0000-0000-000000000001","relative_path":"project"}}"#,
        )
        .unwrap();

        assert_eq!(request.version, PROTOCOL_VERSION);
        assert!(matches!(request.request, Request::WorkspaceOpen(_)));
        assert!(!serde_json::to_string(&request).unwrap().contains("C:\\\\"));
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
        assert_eq!(value["version"], json!(2));
        assert_eq!(value["status"], json!("ok"));
        assert_eq!(value["result"]["type"], json!("exec"));
        assert_eq!(
            serde_json::from_value::<ResponseEnvelope>(value).unwrap(),
            response
        );
    }

    #[test]
    fn stable_errors_use_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::ComputerControlDisabled).unwrap(),
            "\"computer_control_disabled\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::RemotePaused).unwrap(),
            "\"remote_paused\""
        );
    }
}
