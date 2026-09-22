use std::path::Path;

use latch_core::{SessionId, UiRef};

use crate::{
    ApplicationInfo, ApplicationLaunch, AudioState, SemanticElement, UiAction, UiActionResult,
    UiFindQuery, WindowsError, WindowsNative,
};

#[derive(Default)]
pub struct WindowsManager;

impl WindowsManager {
    pub fn new() -> Result<Self, WindowsError> {
        Ok(Self)
    }

    pub fn applications(
        &self,
        _session_id: SessionId,
        _limit: usize,
    ) -> Result<Vec<ApplicationInfo>, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn launch_application(
        &self,
        _program: &str,
        _args: &[String],
        _cwd: Option<&Path>,
    ) -> Result<ApplicationLaunch, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn activate_application(&self, _pid: u32) -> Result<(), WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn quit_application(&self, _pid: u32) -> Result<(), WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn open_target(&self, _target: &str) -> Result<(), WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn clipboard_read(&self) -> Result<String, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn clipboard_write(&self, _text: &str) -> Result<(), WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn audio_state(&self) -> Result<AudioState, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    pub fn audio_set(&self, _percent: u8) -> Result<AudioState, WindowsError> {
        Err(WindowsError::Unsupported)
    }
}

impl WindowsNative for WindowsManager {
    fn desktop_windows(
        &self,
        _session_id: SessionId,
        _limit: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn active_window(&self, _session_id: SessionId) -> Result<SemanticElement, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn subtree(
        &self,
        _session_id: SessionId,
        _root: Option<UiRef>,
        _depth: usize,
        _max_elements: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn find(
        &self,
        _session_id: SessionId,
        _root: Option<UiRef>,
        _query: UiFindQuery,
        _depth: usize,
        _max_results: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn inspect(
        &self,
        _session_id: SessionId,
        _element_ref: UiRef,
    ) -> Result<SemanticElement, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn act(
        &self,
        _session_id: SessionId,
        _element_ref: UiRef,
        _action: UiAction,
    ) -> Result<UiActionResult, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    fn drop_session(&self, _session_id: SessionId) {}
}
