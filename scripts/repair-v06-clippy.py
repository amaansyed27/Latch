from pathlib import Path

# MCP test: prefer sort_by_key with Reverse.
mcp = Path("crates/latch-mcp-client/src/lib.rs")
text = mcp.read_text()
old = "        matches.sort_by(|left, right| right.0.cmp(&left.0));\n"
new = "        matches.sort_by_key(|left| std::cmp::Reverse(left.0));\n"
if text.count(old) != 1:
    raise SystemExit("expected MCP sort fragment not found exactly once")
mcp.write_text(text.replace(old, new, 1))

# Session manager: remove unused hard-delete helper; close() is the supported lifecycle.
session = Path("crates/latch-engine/src/session.rs")
text = session.read_text()
old = """    pub fn remove(&self, session_id: SessionId) -> Result<(), String> {
        lock(&self.sessions).remove(&session_id);
        self.persist()
    }

"""
if text.count(old) != 1:
    raise SystemExit("expected unused SessionManager::remove block not found exactly once")
session.write_text(text.replace(old, "", 1))

# Event coalescing: reuse the existing allocation.
events = Path("crates/latch-engine/src/events.rs")
text = events.read_text()
old = "                last.payload = event.payload.clone();\n"
new = "                last.payload.clone_from(&event.payload);\n"
if text.count(old) != 1:
    raise SystemExit("expected event payload clone assignment not found exactly once")
events.write_text(text.replace(old, new, 1))

engine = Path("crates/latch-engine/src/lib.rs")
text = engine.read_text()
replacements = [
    (
        """use latch_core::{
    ActionId, McpServerId, ProcessId, SessionId, TerminalId, UiRef, Workspace, WorkspaceError,
    WorkspaceId,
};
""",
        """use latch_core::{
    ActionId, McpServerId, ProcessId, SessionId, TerminalId, Workspace, WorkspaceError, WorkspaceId,
};
#[cfg(test)]
use latch_core::UiRef;
""",
    ),
    ("        _config: &LocalConfig,\n", "        config: &LocalConfig,\n"),
    ("                    self.workspace(_config, workspace_id)?;\n", "                    self.workspace(config, workspace_id)?;\n"),
    (
        """                        self.windows_provider()?
                            .active_window(session_id)
                            .map(|window| window.process_id == pid)
                            .unwrap_or(false),
""",
        """                        self.windows_provider()?
                            .active_window(session_id)
                            .is_ok_and(|window| window.process_id == pid),
""",
    ),
    (
        """        let outcome = match (verification_mode, verified) {
            (VerificationMode::None, _) => ActionOutcome::AppliedUnverified,
            (_, Some(true)) => ActionOutcome::Verified,
            (VerificationMode::Required, Some(false) | None) => ActionOutcome::VerificationFailed,
            (VerificationMode::Auto, Some(false)) => ActionOutcome::VerificationFailed,
            (VerificationMode::Auto, None) => ActionOutcome::AppliedUnverified,
        };
""",
        """        let outcome = match (verification_mode, verified) {
            (VerificationMode::None, _) | (VerificationMode::Auto, None) => {
                ActionOutcome::AppliedUnverified
            }
            (_, Some(true)) => ActionOutcome::Verified,
            (VerificationMode::Required, Some(false) | None)
            | (VerificationMode::Auto, Some(false)) => ActionOutcome::VerificationFailed,
        };
""",
    ),
    (
        """        ComputerError::UnsupportedPlatform => ErrorCode::ComputerUnavailable,
        ComputerError::WindowNotFound | ComputerError::InvalidInput(_) => ErrorCode::InvalidRequest,
        ComputerError::ScreenshotTooLarge => ErrorCode::PayloadTooLarge,
        ComputerError::Operation(_) => ErrorCode::ComputerUnavailable,
""",
        """        ComputerError::UnsupportedPlatform | ComputerError::Operation(_) => {
            ErrorCode::ComputerUnavailable
        }
        ComputerError::WindowNotFound | ComputerError::InvalidInput(_) => ErrorCode::InvalidRequest,
        ComputerError::ScreenshotTooLarge => ErrorCode::PayloadTooLarge,
""",
    ),
    (
        """        McpClientError::Protocol(_) => ErrorCode::McpUnavailable,
        McpClientError::Connect(_)
        | McpClientError::MissingEnvironmentReference(_)
        | McpClientError::Runtime(_)
        | McpClientError::ManagerStopped => ErrorCode::McpUnavailable,
""",
        """        McpClientError::Protocol(_)
        | McpClientError::Connect(_)
        | McpClientError::MissingEnvironmentReference(_)
        | McpClientError::Runtime(_)
        | McpClientError::ManagerStopped => ErrorCode::McpUnavailable,
""",
    ),
]
for old, new in replacements:
    if text.count(old) != 1:
        raise SystemExit(f"expected engine fragment not found exactly once: {old[:100]!r}")
    text = text.replace(old, new, 1)

# These are deliberately broad domain dispatchers over bounded request enums. Keep the
# exception local rather than weakening the workspace's clippy policy.
for signature in [
    "    fn agent_inspect(\n",
    "    fn agent_files(\n",
    "    fn agent_exec(\n",
    "    fn agent_act(&self, config: &LocalConfig, request: ActRequest) -> Result<Value, ProtocolError> {\n",
    "    fn agent_browser(\n",
]:
    if text.count(signature) != 1:
        raise SystemExit(f"expected dispatcher signature not found exactly once: {signature!r}")
    text = text.replace(signature, "    #[allow(clippy::too_many_lines)]\n" + signature, 1)

engine.write_text(text)
