use latch_core::{
    BrowserContextId, McpServerId, ProcessId, RootId, SessionId, TabId, TerminalId, ToolRefId,
    UiRef, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "domain", content = "request", rename_all = "snake_case")]
pub enum AgentRequest {
    Session(SessionRequest),
    Inspect(InspectRequest),
    Files(FilesRequest),
    Exec(ExecDomainRequest),
    Act(ActRequest),
    Browser(BrowserRequest),
    Tools(ToolsRequest),
    Events(EventsRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SessionRequest {
    Create,
    Inspect { session_id: SessionId },
    Update {
        session_id: SessionId,
        #[serde(default)]
        workspace_ids: Vec<WorkspaceId>,
    },
    Close { session_id: SessionId },
    Cancel { session_id: SessionId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum InspectRequest {
    Windows {
        session_id: SessionId,
        #[serde(default)]
        limit: Option<usize>,
    },
    ActiveWindow { session_id: SessionId },
    UiTree {
        session_id: SessionId,
        #[serde(default)]
        root: Option<UiRef>,
        #[serde(default)]
        depth: Option<usize>,
        #[serde(default)]
        max_elements: Option<usize>,
    },
    UiFind {
        session_id: SessionId,
        #[serde(default)]
        root: Option<UiRef>,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        automation_id: Option<String>,
        #[serde(default)]
        exact_name: bool,
        #[serde(default)]
        depth: Option<usize>,
        #[serde(default)]
        max_results: Option<usize>,
    },
    UiRef {
        session_id: SessionId,
        element_ref: UiRef,
    },
    Applications {
        session_id: SessionId,
        #[serde(default)]
        limit: Option<usize>,
    },
    Audio { session_id: SessionId },
    Clipboard { session_id: SessionId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FilesRequest {
    Roots,
    OpenWorkspace {
        root_id: RootId,
        #[serde(default)]
        relative_path: Option<String>,
    },
    List { workspace_id: WorkspaceId, path: String },
    Stat { workspace_id: WorkspaceId, path: String },
    Read {
        workspace_id: WorkspaceId,
        path: String,
        #[serde(default)]
        max_bytes: Option<usize>,
    },
    Write {
        workspace_id: WorkspaceId,
        path: String,
        contents: String,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        verification: VerificationMode,
    },
    Patch {
        workspace_id: WorkspaceId,
        path: String,
        replacements: Vec<FileReplacement>,
        #[serde(default)]
        verification: VerificationMode,
    },
    Search {
        workspace_id: WorkspaceId,
        query: String,
        #[serde(default)]
        filename: bool,
        #[serde(default = "default_true")]
        content: bool,
        #[serde(default)]
        glob: Option<String>,
        #[serde(default)]
        max_results: Option<usize>,
    },
    Mkdir { workspace_id: WorkspaceId, path: String },
    Move {
        workspace_id: WorkspaceId,
        from: String,
        to: String,
        #[serde(default)]
        overwrite: bool,
    },
    Delete { workspace_id: WorkspaceId, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileReplacement {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ExecDomainRequest {
    Run {
        session_id: SessionId,
        workspace_id: WorkspaceId,
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Start {
        session_id: SessionId,
        workspace_id: WorkspaceId,
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Poll { session_id: SessionId, job_id: ProcessId },
    Stdin {
        session_id: SessionId,
        job_id: ProcessId,
        #[serde(default)]
        text: String,
        #[serde(default)]
        close_stdin: bool,
    },
    Kill { session_id: SessionId, job_id: ProcessId },
    TerminalProfiles { session_id: SessionId },
    TerminalCreate {
        session_id: SessionId,
        workspace_id: WorkspaceId,
        #[serde(default)]
        profile_id: Option<String>,
        #[serde(default)]
        rows: Option<u16>,
        #[serde(default)]
        cols: Option<u16>,
    },
    TerminalWrite {
        session_id: SessionId,
        terminal_id: TerminalId,
        text: String,
    },
    TerminalRead {
        session_id: SessionId,
        terminal_id: TerminalId,
        #[serde(default)]
        after_sequence: Option<u64>,
        #[serde(default)]
        max_bytes: Option<usize>,
    },
    TerminalResize {
        session_id: SessionId,
        terminal_id: TerminalId,
        rows: u16,
        cols: u16,
    },
    TerminalInterrupt {
        session_id: SessionId,
        terminal_id: TerminalId,
    },
    TerminalKill {
        session_id: SessionId,
        terminal_id: TerminalId,
    },
    TerminalList { session_id: SessionId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ActRequest {
    AppLaunch {
        session_id: SessionId,
        program: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        workspace_id: Option<WorkspaceId>,
        #[serde(default)]
        verification: VerificationMode,
    },
    AppActivate {
        session_id: SessionId,
        pid: u32,
        #[serde(default)]
        verification: VerificationMode,
    },
    AppQuit {
        session_id: SessionId,
        pid: u32,
        #[serde(default)]
        verification: VerificationMode,
    },
    OpenTarget { session_id: SessionId, target: String },
    ClipboardWrite {
        session_id: SessionId,
        text: String,
        #[serde(default)]
        verification: VerificationMode,
    },
    AudioSet {
        session_id: SessionId,
        volume_percent: u8,
        #[serde(default)]
        verification: VerificationMode,
    },
    Ui {
        session_id: SessionId,
        element_ref: UiRef,
        action: UiActionRequest,
        #[serde(default)]
        verification: VerificationMode,
    },
    Screenshot {
        session_id: SessionId,
        #[serde(default)]
        display_id: Option<String>,
        #[serde(default)]
        format: Option<String>,
    },
    RawInput {
        session_id: SessionId,
        input: RawInputRequest,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UiActionRequest {
    Invoke,
    SetValue { value: String },
    Select,
    Toggle,
    Expand,
    Collapse,
    Scroll,
    Focus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RawInputRequest {
    MouseMove { x: i32, y: i32 },
    MouseClick {
        #[serde(default = "default_left")]
        button: String,
    },
    MouseDrag {
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        #[serde(default = "default_left")]
        button: String,
    },
    Scroll {
        amount: i32,
        #[serde(default = "default_vertical")]
        axis: String,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    Type { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserRequest {
    Status { session_id: SessionId },
    CreateContext {
        session_id: SessionId,
        #[serde(default)]
        authenticated: bool,
        #[serde(default)]
        persistent: bool,
    },
    ListContexts { session_id: SessionId },
    CloseContext {
        session_id: SessionId,
        context_id: BrowserContextId,
    },
    NewTab {
        session_id: SessionId,
        context_id: BrowserContextId,
        #[serde(default)]
        url: Option<String>,
    },
    ListTabs {
        session_id: SessionId,
        #[serde(default)]
        context_id: Option<BrowserContextId>,
    },
    CloseTab { session_id: SessionId, tab_id: TabId },
    Navigate {
        session_id: SessionId,
        tab_id: TabId,
        url: String,
    },
    Snapshot { session_id: SessionId, tab_id: TabId },
    Find {
        session_id: SessionId,
        tab_id: TabId,
        target: Value,
        #[serde(default)]
        max_results: Option<usize>,
    },
    Act {
        session_id: SessionId,
        tab_id: TabId,
        target: Value,
        action: Value,
        #[serde(default)]
        browser_verification: Option<Value>,
        #[serde(default)]
        verification: VerificationMode,
    },
    Console {
        session_id: SessionId,
        tab_id: TabId,
        #[serde(default)]
        after_sequence: u64,
        #[serde(default)]
        max_entries: Option<usize>,
    },
    Network {
        session_id: SessionId,
        tab_id: TabId,
        #[serde(default)]
        after_sequence: u64,
        #[serde(default)]
        max_entries: Option<usize>,
    },
    Downloads {
        session_id: SessionId,
        tab_id: TabId,
        #[serde(default)]
        after_sequence: u64,
        #[serde(default)]
        max_entries: Option<usize>,
    },
    Screenshot { session_id: SessionId, tab_id: TabId },
    PageState { session_id: SessionId, tab_id: TabId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ToolsRequest {
    Providers { session_id: SessionId },
    Search {
        session_id: SessionId,
        query: String,
        #[serde(default)]
        provider_id: Option<McpServerId>,
        #[serde(default)]
        max_results: Option<usize>,
    },
    Describe { session_id: SessionId, tool_ref: ToolRefId },
    Call {
        session_id: SessionId,
        tool_ref: ToolRefId,
        #[serde(default = "empty_object")]
        arguments: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventsRequest {
    pub session_id: SessionId,
    #[serde(default)]
    pub after_sequence: u64,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub wait_ms: u64,
    #[serde(default)]
    pub max_events: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMode {
    #[default]
    Auto,
    Required,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Verified,
    AppliedUnverified,
    NoChange,
    Failed,
    VerificationFailed,
    PermissionDenied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifiedActionResponse {
    pub action_id: latch_core::ActionId,
    pub route_used: String,
    pub outcome: ActionOutcome,
    pub verification: Option<Value>,
    #[serde(default)]
    pub changed_refs: Vec<String>,
    pub revision: u64,
    #[serde(default)]
    pub result: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::default())
}

fn default_true() -> bool {
    true
}

fn default_left() -> String {
    "left".to_owned()
}

fn default_vertical() -> String {
    "vertical".to_owned()
}
