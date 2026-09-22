mod id;
mod workspace;

pub use id::{
    ActionId, ApprovalId, BrowserContextId, DeviceId, McpServerId, ProcessId, RootId, SessionId,
    TabId, TerminalId, ToolRefId, UiRef, WorkspaceId,
};
pub use workspace::{Workspace, WorkspaceError};
