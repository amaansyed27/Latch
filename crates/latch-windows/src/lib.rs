use latch_core::{SessionId, UiRef};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(windows)]
mod platform;
#[cfg(not(windows))]
mod stub;

#[cfg(windows)]
pub use platform::WindowsManager;
#[cfg(not(windows))]
pub use stub::WindowsManager;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticElement {
    pub element_ref: UiRef,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub automation_id: String,
    pub bounds: UiBounds,
    pub enabled: bool,
    pub focused: bool,
    pub offscreen: bool,
    pub process_id: u32,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiFindQuery {
    pub role: Option<String>,
    pub name: Option<String>,
    pub automation_id: Option<String>,
    #[serde(default)]
    pub exact_name: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UiAction {
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
pub struct UiActionResult {
    pub element: Option<SemanticElement>,
    pub deterministic_verification: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplicationInfo {
    pub pid: u32,
    pub process_name: String,
    pub title: String,
    pub window_ref: Option<UiRef>,
    pub focused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplicationLaunch {
    pub pid: u32,
    pub program: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioState {
    pub volume_percent: u8,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WindowsError {
    #[error("Windows native capability is unavailable on this platform")]
    Unsupported,
    #[error("Windows UI Automation is unavailable: {0}")]
    UiUnavailable(String),
    #[error("UI reference is stale")]
    StaleRef,
    #[error("UI reference does not belong to this session")]
    RefSessionMismatch,
    #[error("target is elevated and cannot be automated from the current integrity level")]
    TargetElevated,
    #[error("secure desktop and UAC surfaces are not automatable by Latch")]
    SecureDesktop,
    #[error("application operation failed: {0}")]
    Application(String),
    #[error("clipboard operation failed: {0}")]
    Clipboard(String),
    #[error("audio operation failed: {0}")]
    Audio(String),
    #[error("invalid Windows operation: {0}")]
    InvalidInput(String),
    #[error("Windows capability worker stopped")]
    WorkerStopped,
}

pub trait WindowsNative {
    fn desktop_windows(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError>;
    fn active_window(&self, session_id: SessionId) -> Result<SemanticElement, WindowsError>;
    fn subtree(
        &self,
        session_id: SessionId,
        root: Option<UiRef>,
        depth: usize,
        max_elements: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError>;
    fn find(
        &self,
        session_id: SessionId,
        root: Option<UiRef>,
        query: UiFindQuery,
        depth: usize,
        max_results: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError>;
    fn inspect(
        &self,
        session_id: SessionId,
        element_ref: UiRef,
    ) -> Result<SemanticElement, WindowsError>;
    fn act(
        &self,
        session_id: SessionId,
        element_ref: UiRef,
        action: UiAction,
    ) -> Result<UiActionResult, WindowsError>;
    fn drop_session(&self, session_id: SessionId);
}
