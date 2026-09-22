use std::{
    collections::HashMap,
    path::Path,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use latch_core::{SessionId, UiRef};
use tracing::warn;
use uiautomation::{
    clipboards::Clipboard,
    patterns::{
        UIExpandCollapsePattern, UIInvokePattern, UIScrollItemPattern, UISelectionItemPattern,
        UITogglePattern, UIValuePattern, UIWindowPattern,
    },
    types::{ControlType, ExpandCollapseState},
    UIAutomation, UIElement, UITreeWalker,
};

use crate::{
    ApplicationInfo, ApplicationLaunch, AudioState, SemanticElement, UiAction, UiActionResult,
    UiBounds, UiFindQuery, WindowsError, WindowsNative,
};

const MAX_WINDOWS: usize = 64;
const MAX_DEPTH: usize = 8;
const MAX_ELEMENTS: usize = 256;
const MAX_RESULTS: usize = 64;
const MAX_TEXT_CHARS: usize = 64 * 1024;

pub struct WindowsManager {
    sender: Sender<WorkerCommand>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsManager {
    pub fn new() -> Result<Self, WindowsError> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("latch-uia".to_owned())
            .spawn(move || run_worker(receiver))
            .map_err(|error| WindowsError::UiUnavailable(error.to_string()))?;
        let manager = Self {
            sender,
            worker: Some(worker),
        };
        manager.ping()?;
        Ok(manager)
    }

    pub fn applications(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Result<Vec<ApplicationInfo>, WindowsError> {
        let windows = self.desktop_windows(session_id, limit.min(MAX_WINDOWS))?;
        Ok(windows
            .into_iter()
            .map(|window| ApplicationInfo {
                pid: window.process_id,
                process_name: process_name(window.process_id).unwrap_or_default(),
                title: window.name,
                window_ref: Some(window.element_ref),
                focused: window.focused,
            })
            .collect())
    }

    pub fn launch_application(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<ApplicationLaunch, WindowsError> {
        if program.trim().is_empty() || program.chars().count() > 4096 || args.len() > 256 {
            return Err(WindowsError::InvalidInput(
                "invalid application program or argument count".to_owned(),
            ));
        }
        if args.iter().any(|arg| arg.chars().count() > 8192) {
            return Err(WindowsError::InvalidInput(
                "application argument is too large".to_owned(),
            ));
        }
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let child = command
            .spawn()
            .map_err(|error| WindowsError::Application(error.to_string()))?;
        Ok(ApplicationLaunch {
            pid: child.id(),
            program: program.to_owned(),
        })
    }

    pub fn activate_application(&self, pid: u32) -> Result<(), WindowsError> {
        self.request(|reply| WorkerCommand::ActivatePid { pid, reply })
    }

    pub fn quit_application(&self, pid: u32) -> Result<(), WindowsError> {
        match self.request(|reply| WorkerCommand::ClosePid { pid, reply }) {
            Ok(()) => Ok(()),
            Err(WindowsError::UiUnavailable(_)) => {
                let status = Command::new("taskkill.exe")
                    .args(["/PID", &pid.to_string(), "/T"])
                    .status()
                    .map_err(|error| WindowsError::Application(error.to_string()))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(WindowsError::Application(format!(
                        "taskkill exited with {status}"
                    )))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn open_target(&self, target: &str) -> Result<(), WindowsError> {
        if target.trim().is_empty() || target.chars().count() > 8192 {
            return Err(WindowsError::InvalidInput(
                "open target must contain 1 to 8192 characters".to_owned(),
            ));
        }
        Command::new("explorer.exe")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| WindowsError::Application(error.to_string()))
    }

    pub fn clipboard_read(&self) -> Result<String, WindowsError> {
        Clipboard::open()
            .and_then(|clipboard| clipboard.get_text())
            .map_err(|error| WindowsError::Clipboard(error.to_string()))
    }

    pub fn clipboard_write(&self, text: &str) -> Result<(), WindowsError> {
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(WindowsError::InvalidInput(
                "clipboard text exceeds the local limit".to_owned(),
            ));
        }
        Clipboard::open()
            .and_then(|clipboard| clipboard.set_text(text))
            .map_err(|error| WindowsError::Clipboard(error.to_string()))
    }

    pub fn audio_state(&self) -> Result<AudioState, WindowsError> {
        Ok(AudioState {
            volume_percent: cpvc::get_system_volume(),
        })
    }

    pub fn audio_set(&self, percent: u8) -> Result<AudioState, WindowsError> {
        if percent > 100 {
            return Err(WindowsError::InvalidInput(
                "volume must be between 0 and 100".to_owned(),
            ));
        }
        if !cpvc::set_system_volume(percent) {
            return Err(WindowsError::Audio(
                "Windows did not accept the requested volume".to_owned(),
            ));
        }
        let state = self.audio_state()?;
        if state.volume_percent.abs_diff(percent) > 1 {
            return Err(WindowsError::Audio(format!(
                "volume verification failed: requested {percent}, observed {}",
                state.volume_percent
            )));
        }
        Ok(state)
    }

    fn ping(&self) -> Result<(), WindowsError> {
        self.request(WorkerCommand::Ping)
    }

    fn request<T: Send + 'static>(
        &self,
        build: impl FnOnce(Sender<Result<T, WindowsError>>) -> WorkerCommand,
    ) -> Result<T, WindowsError> {
        let (reply, receive) = mpsc::channel();
        self.sender
            .send(build(reply))
            .map_err(|_| WindowsError::WorkerStopped)?;
        receive.recv().map_err(|_| WindowsError::WorkerStopped)?
    }
}

impl WindowsNative for WindowsManager {
    fn desktop_windows(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        self.request(|reply| WorkerCommand::DesktopWindows {
            session_id,
            limit: limit.clamp(1, MAX_WINDOWS),
            reply,
        })
    }

    fn active_window(&self, session_id: SessionId) -> Result<SemanticElement, WindowsError> {
        self.request(|reply| WorkerCommand::ActiveWindow { session_id, reply })
    }

    fn subtree(
        &self,
        session_id: SessionId,
        root: Option<UiRef>,
        depth: usize,
        max_elements: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        self.request(|reply| WorkerCommand::Subtree {
            session_id,
            root,
            depth: depth.clamp(0, MAX_DEPTH),
            max_elements: max_elements.clamp(1, MAX_ELEMENTS),
            reply,
        })
    }

    fn find(
        &self,
        session_id: SessionId,
        root: Option<UiRef>,
        query: UiFindQuery,
        depth: usize,
        max_results: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        self.request(|reply| WorkerCommand::Find {
            session_id,
            root,
            query,
            depth: depth.clamp(0, MAX_DEPTH),
            max_results: max_results.clamp(1, MAX_RESULTS),
            reply,
        })
    }

    fn inspect(
        &self,
        session_id: SessionId,
        element_ref: UiRef,
    ) -> Result<SemanticElement, WindowsError> {
        self.request(|reply| WorkerCommand::Inspect {
            session_id,
            element_ref,
            reply,
        })
    }

    fn act(
        &self,
        session_id: SessionId,
        element_ref: UiRef,
        action: UiAction,
    ) -> Result<UiActionResult, WindowsError> {
        self.request(|reply| WorkerCommand::Act {
            session_id,
            element_ref,
            action,
            reply,
        })
    }

    fn drop_session(&self, session_id: SessionId) {
        let _ = self.sender.send(WorkerCommand::DropSession { session_id });
    }
}

impl Drop for WindowsManager {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

enum WorkerCommand {
    Ping(Sender<Result<(), WindowsError>>),
    DesktopWindows {
        session_id: SessionId,
        limit: usize,
        reply: Sender<Result<Vec<SemanticElement>, WindowsError>>,
    },
    ActiveWindow {
        session_id: SessionId,
        reply: Sender<Result<SemanticElement, WindowsError>>,
    },
    Subtree {
        session_id: SessionId,
        root: Option<UiRef>,
        depth: usize,
        max_elements: usize,
        reply: Sender<Result<Vec<SemanticElement>, WindowsError>>,
    },
    Find {
        session_id: SessionId,
        root: Option<UiRef>,
        query: UiFindQuery,
        depth: usize,
        max_results: usize,
        reply: Sender<Result<Vec<SemanticElement>, WindowsError>>,
    },
    Inspect {
        session_id: SessionId,
        element_ref: UiRef,
        reply: Sender<Result<SemanticElement, WindowsError>>,
    },
    Act {
        session_id: SessionId,
        element_ref: UiRef,
        action: UiAction,
        reply: Sender<Result<UiActionResult, WindowsError>>,
    },
    ActivatePid {
        pid: u32,
        reply: Sender<Result<(), WindowsError>>,
    },
    ClosePid {
        pid: u32,
        reply: Sender<Result<(), WindowsError>>,
    },
    DropSession {
        session_id: SessionId,
    },
    Shutdown,
}

struct RefRecord {
    session_id: SessionId,
    runtime_id: Vec<i32>,
    element: UIElement,
}

struct UiWorker {
    automation: UIAutomation,
    walker: UITreeWalker,
    refs: HashMap<UiRef, RefRecord>,
    runtime_refs: HashMap<(SessionId, Vec<i32>), UiRef>,
}

fn run_worker(receiver: Receiver<WorkerCommand>) {
    let initialized = UIAutomation::new()
        .and_then(|automation| automation.get_control_view_walker().map(|walker| (automation, walker)));
    let Ok((automation, walker)) = initialized else {
        let message = initialized
            .err()
            .map_or_else(|| "unknown UIA initialization failure".to_owned(), |error| error.to_string());
        fail_worker(receiver, WindowsError::UiUnavailable(message));
        return;
    };
    let mut worker = UiWorker {
        automation,
        walker,
        refs: HashMap::new(),
        runtime_refs: HashMap::new(),
    };
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Ping(reply) => {
                let _ = reply.send(Ok(()));
            }
            WorkerCommand::DesktopWindows {
                session_id,
                limit,
                reply,
            } => {
                let _ = reply.send(worker.desktop_windows(session_id, limit));
            }
            WorkerCommand::ActiveWindow { session_id, reply } => {
                let _ = reply.send(worker.active_window(session_id));
            }
            WorkerCommand::Subtree {
                session_id,
                root,
                depth,
                max_elements,
                reply,
            } => {
                let _ = reply.send(worker.subtree(session_id, root, depth, max_elements));
            }
            WorkerCommand::Find {
                session_id,
                root,
                query,
                depth,
                max_results,
                reply,
            } => {
                let _ = reply.send(worker.find(session_id, root, &query, depth, max_results));
            }
            WorkerCommand::Inspect {
                session_id,
                element_ref,
                reply,
            } => {
                let _ = reply.send(worker.inspect(session_id, element_ref));
            }
            WorkerCommand::Act {
                session_id,
                element_ref,
                action,
                reply,
            } => {
                let _ = reply.send(worker.act(session_id, element_ref, action));
            }
            WorkerCommand::ActivatePid { pid, reply } => {
                let _ = reply.send(worker.activate_pid(pid));
            }
            WorkerCommand::ClosePid { pid, reply } => {
                let _ = reply.send(worker.close_pid(pid));
            }
            WorkerCommand::DropSession { session_id } => worker.drop_session(session_id),
            WorkerCommand::Shutdown => return,
        }
    }
}

fn fail_worker(receiver: Receiver<WorkerCommand>, error: WindowsError) {
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Ping(reply) => {
                let _ = reply.send(Err(error.clone()));
            }
            WorkerCommand::DesktopWindows { reply, .. }
            | WorkerCommand::Subtree { reply, .. }
            | WorkerCommand::Find { reply, .. } => {
                let _ = reply.send(Err(error.clone()));
            }
            WorkerCommand::ActiveWindow { reply, .. } | WorkerCommand::Inspect { reply, .. } => {
                let _ = reply.send(Err(error.clone()));
            }
            WorkerCommand::Act { reply, .. } => {
                let _ = reply.send(Err(error.clone()));
            }
            WorkerCommand::ActivatePid { reply, .. } | WorkerCommand::ClosePid { reply, .. } => {
                let _ = reply.send(Err(error.clone()));
            }
            WorkerCommand::DropSession { .. } => {}
            WorkerCommand::Shutdown => return,
        }
    }
}

impl UiWorker {
    fn desktop_windows(
        &mut self,
        session_id: SessionId,
        limit: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        let root = self.automation.get_root_element().map_err(map_uia_error)?;
        let mut result = Vec::new();
        let Ok(mut current) = self.walker.get_first_child(&root) else {
            return Ok(result);
        };
        loop {
            if current.get_control_type().map_err(map_uia_error)? == ControlType::Window {
                result.push(self.semantic(session_id, &current)?);
                if result.len() >= limit {
                    break;
                }
            }
            match self.walker.get_next_sibling(&current) {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
        Ok(result)
    }

    fn active_window(&mut self, session_id: SessionId) -> Result<SemanticElement, WindowsError> {
        let mut current = self.automation.get_focused_element().map_err(map_uia_error)?;
        for _ in 0..16 {
            if current.get_control_type().map_err(map_uia_error)? == ControlType::Window {
                return self.semantic(session_id, &current);
            }
            current = self
                .walker
                .get_parent(&current)
                .map_err(map_uia_error)?;
        }
        self.semantic(session_id, &current)
    }

    fn subtree(
        &mut self,
        session_id: SessionId,
        root: Option<UiRef>,
        depth: usize,
        max_elements: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        let root = self.resolve_root(session_id, root)?;
        let mut elements = Vec::new();
        self.walk_bounded(session_id, &root, depth, max_elements, &mut elements)?;
        Ok(elements)
    }

    fn find(
        &mut self,
        session_id: SessionId,
        root: Option<UiRef>,
        query: &UiFindQuery,
        depth: usize,
        max_results: usize,
    ) -> Result<Vec<SemanticElement>, WindowsError> {
        let root = self.resolve_root(session_id, root)?;
        let mut all = Vec::new();
        self.walk_bounded(session_id, &root, depth, MAX_ELEMENTS, &mut all)?;
        Ok(all
            .into_iter()
            .filter(|element| matches_query(element, query))
            .take(max_results)
            .collect())
    }

    fn inspect(
        &mut self,
        session_id: SessionId,
        element_ref: UiRef,
    ) -> Result<SemanticElement, WindowsError> {
        let element = self.element(session_id, element_ref)?.clone();
        self.semantic_with_ref(element_ref, &element)
    }

    fn act(
        &mut self,
        session_id: SessionId,
        element_ref: UiRef,
        action: UiAction,
    ) -> Result<UiActionResult, WindowsError> {
        let element = self.element(session_id, element_ref)?.clone();
        reject_secure_desktop(element.get_process_id().map_err(map_uia_error)?)?;
        let verification = match action {
            UiAction::Invoke => {
                element
                    .get_pattern::<UIInvokePattern>()
                    .and_then(|pattern| pattern.invoke())
                    .map_err(map_uia_error)?;
                None
            }
            UiAction::SetValue { value } => {
                if value.chars().count() > MAX_TEXT_CHARS {
                    return Err(WindowsError::InvalidInput(
                        "UI value exceeds the local limit".to_owned(),
                    ));
                }
                let pattern = element
                    .get_pattern::<UIValuePattern>()
                    .map_err(map_uia_error)?;
                pattern.set_value(&value).map_err(map_uia_error)?;
                Some(pattern.get_value().map_err(map_uia_error)? == value)
            }
            UiAction::Select => {
                let pattern = element
                    .get_pattern::<UISelectionItemPattern>()
                    .map_err(map_uia_error)?;
                pattern.select().map_err(map_uia_error)?;
                Some(pattern.is_selected().map_err(map_uia_error)?)
            }
            UiAction::Toggle => {
                let pattern = element
                    .get_pattern::<UITogglePattern>()
                    .map_err(map_uia_error)?;
                let before = pattern.get_toggle_state().map_err(map_uia_error)?;
                pattern.toggle().map_err(map_uia_error)?;
                Some(pattern.get_toggle_state().map_err(map_uia_error)? != before)
            }
            UiAction::Expand => {
                let pattern = element
                    .get_pattern::<UIExpandCollapsePattern>()
                    .map_err(map_uia_error)?;
                pattern.expand().map_err(map_uia_error)?;
                Some(matches!(
                    pattern.get_state().map_err(map_uia_error)?,
                    ExpandCollapseState::Expanded | ExpandCollapseState::PartiallyExpanded
                ))
            }
            UiAction::Collapse => {
                let pattern = element
                    .get_pattern::<UIExpandCollapsePattern>()
                    .map_err(map_uia_error)?;
                pattern.collapse().map_err(map_uia_error)?;
                Some(matches!(
                    pattern.get_state().map_err(map_uia_error)?,
                    ExpandCollapseState::Collapsed
                ))
            }
            UiAction::Scroll => {
                element
                    .get_pattern::<UIScrollItemPattern>()
                    .and_then(|pattern| pattern.scroll_into_view())
                    .map_err(map_uia_error)?;
                Some(!element.is_offscreen().map_err(map_uia_error)?)
            }
            UiAction::Focus => {
                element.set_focus().map_err(map_uia_error)?;
                Some(element.has_keyboard_focus().map_err(map_uia_error)?)
            }
        };
        Ok(UiActionResult {
            element: Some(self.semantic_with_ref(element_ref, &element)?),
            deterministic_verification: verification,
        })
    }

    fn activate_pid(&mut self, pid: u32) -> Result<(), WindowsError> {
        reject_secure_desktop(pid)?;
        let window = self.window_for_pid(pid)?;
        window.set_focus().map_err(map_uia_error)
    }

    fn close_pid(&mut self, pid: u32) -> Result<(), WindowsError> {
        reject_secure_desktop(pid)?;
        let window = self.window_for_pid(pid)?;
        window
            .get_pattern::<UIWindowPattern>()
            .and_then(|pattern| pattern.close())
            .map_err(map_uia_error)
    }

    fn window_for_pid(&self, pid: u32) -> Result<UIElement, WindowsError> {
        let root = self.automation.get_root_element().map_err(map_uia_error)?;
        let mut current = self.walker.get_first_child(&root).map_err(map_uia_error)?;
        loop {
            if current.get_process_id().map_err(map_uia_error)? == pid
                && current.get_control_type().map_err(map_uia_error)? == ControlType::Window
            {
                return Ok(current);
            }
            current = self
                .walker
                .get_next_sibling(&current)
                .map_err(|_| WindowsError::UiUnavailable(format!("no top-level window for pid {pid}")))?;
        }
    }

    fn resolve_root(
        &mut self,
        session_id: SessionId,
        root: Option<UiRef>,
    ) -> Result<UIElement, WindowsError> {
        match root {
            Some(root) => Ok(self.element(session_id, root)?.clone()),
            None => self.automation.get_root_element().map_err(map_uia_error),
        }
    }

    fn walk_bounded(
        &mut self,
        session_id: SessionId,
        root: &UIElement,
        depth: usize,
        max_elements: usize,
        result: &mut Vec<SemanticElement>,
    ) -> Result<(), WindowsError> {
        if result.len() >= max_elements {
            return Ok(());
        }
        result.push(self.semantic(session_id, root)?);
        if depth == 0 || result.len() >= max_elements {
            return Ok(());
        }
        let Ok(mut current) = self.walker.get_first_child(root) else {
            return Ok(());
        };
        loop {
            self.walk_bounded(session_id, &current, depth - 1, max_elements, result)?;
            if result.len() >= max_elements {
                break;
            }
            match self.walker.get_next_sibling(&current) {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
        Ok(())
    }

    fn semantic(
        &mut self,
        session_id: SessionId,
        element: &UIElement,
    ) -> Result<SemanticElement, WindowsError> {
        let element_ref = self.register(session_id, element)?;
        self.semantic_with_ref(element_ref, element)
    }

    fn semantic_with_ref(
        &self,
        element_ref: UiRef,
        element: &UIElement,
    ) -> Result<SemanticElement, WindowsError> {
        let bounds = element.get_bounding_rectangle().map_err(map_uia_error)?;
        let mut actions = Vec::with_capacity(7);
        if element.get_pattern::<UIInvokePattern>().is_ok() {
            actions.push("invoke".to_owned());
        }
        if element.get_pattern::<UIValuePattern>().is_ok() {
            actions.push("set_value".to_owned());
        }
        if element.get_pattern::<UISelectionItemPattern>().is_ok() {
            actions.push("select".to_owned());
        }
        if element.get_pattern::<UITogglePattern>().is_ok() {
            actions.push("toggle".to_owned());
        }
        if element.get_pattern::<UIExpandCollapsePattern>().is_ok() {
            actions.push("expand".to_owned());
            actions.push("collapse".to_owned());
        }
        if element.get_pattern::<UIScrollItemPattern>().is_ok() {
            actions.push("scroll".to_owned());
        }
        if element.is_keyboard_focusable().unwrap_or(false) {
            actions.push("focus".to_owned());
        }
        let value = if element.is_password().unwrap_or(false) {
            None
        } else {
            element
                .get_pattern::<UIValuePattern>()
                .ok()
                .and_then(|pattern| pattern.get_value().ok())
        };
        let role = element
            .get_localized_control_type()
            .unwrap_or_else(|_| format!("{:?}", element.get_control_type().unwrap_or(ControlType::Custom)));
        Ok(SemanticElement {
            element_ref,
            role: role.to_ascii_lowercase(),
            name: bounded(element.get_name().map_err(map_uia_error)?, 1024),
            value: value.map(|value| bounded(value, 4096)),
            automation_id: bounded(element.get_automation_id().unwrap_or_default(), 512),
            bounds: UiBounds {
                x: bounds.get_left(),
                y: bounds.get_top(),
                width: bounds.get_width().max(0),
                height: bounds.get_height().max(0),
            },
            enabled: element.is_enabled().unwrap_or(false),
            focused: element.has_keyboard_focus().unwrap_or(false),
            offscreen: element.is_offscreen().unwrap_or(true),
            process_id: element.get_process_id().map_err(map_uia_error)?,
            actions,
        })
    }

    fn register(
        &mut self,
        session_id: SessionId,
        element: &UIElement,
    ) -> Result<UiRef, WindowsError> {
        let runtime_id = element.get_runtime_id().map_err(map_uia_error)?;
        let key = (session_id, runtime_id.clone());
        if let Some(existing) = self.runtime_refs.get(&key).copied() {
            if self.refs.contains_key(&existing) {
                return Ok(existing);
            }
        }
        let element_ref = UiRef::new();
        self.refs.insert(
            element_ref,
            RefRecord {
                session_id,
                runtime_id: runtime_id.clone(),
                element: element.clone(),
            },
        );
        self.runtime_refs.insert(key, element_ref);
        Ok(element_ref)
    }

    fn element(&mut self, session_id: SessionId, element_ref: UiRef) -> Result<&UIElement, WindowsError> {
        let Some(record) = self.refs.get(&element_ref) else {
            return Err(WindowsError::StaleRef);
        };
        if record.session_id != session_id {
            return Err(WindowsError::RefSessionMismatch);
        }
        match record.element.get_runtime_id() {
            Ok(runtime_id) if runtime_id == record.runtime_id => Ok(&record.element),
            _ => {
                self.refs.remove(&element_ref);
                Err(WindowsError::StaleRef)
            }
        }
    }

    fn drop_session(&mut self, session_id: SessionId) {
        self.refs.retain(|_, record| record.session_id != session_id);
        self.runtime_refs
            .retain(|(candidate, _), _| *candidate != session_id);
    }
}

fn matches_query(element: &SemanticElement, query: &UiFindQuery) -> bool {
    if let Some(role) = query.role.as_deref() {
        if !element.role.eq_ignore_ascii_case(role) {
            return false;
        }
    }
    if let Some(automation_id) = query.automation_id.as_deref() {
        if !element.automation_id.eq_ignore_ascii_case(automation_id) {
            return false;
        }
    }
    if let Some(name) = query.name.as_deref() {
        if query.exact_name {
            if !element.name.eq_ignore_ascii_case(name) {
                return false;
            }
        } else if !element
            .name
            .to_ascii_lowercase()
            .contains(&name.to_ascii_lowercase())
        {
            return false;
        }
    }
    true
}

fn reject_secure_desktop(pid: u32) -> Result<(), WindowsError> {
    if process_name(pid).is_some_and(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "consent.exe" | "winlogon.exe" | "logonui.exe"
        )
    }) {
        Err(WindowsError::SecureDesktop)
    } else {
        Ok(())
    }
}

fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("tasklist.exe")
        .args([
            "/FI",
            &format!("PID eq {pid}"),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    if first.starts_with("INFO:") {
        return None;
    }
    first
        .split(',')
        .next()
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
}

fn map_uia_error(error: uiautomation::Error) -> WindowsError {
    let text = error.to_string();
    let normalized = text.to_ascii_lowercase();
    if normalized.contains("0x80070005") || normalized.contains("access is denied") || normalized.contains("access denied") {
        WindowsError::TargetElevated
    } else {
        WindowsError::UiUnavailable(text)
    }
}

fn bounded(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_query_is_case_insensitive_and_bounded() {
        let element = SemanticElement {
            element_ref: UiRef::new(),
            role: "button".to_owned(),
            name: "Save document".to_owned(),
            value: None,
            automation_id: "save".to_owned(),
            bounds: UiBounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            enabled: true,
            focused: false,
            offscreen: false,
            process_id: 1,
            actions: vec!["invoke".to_owned()],
        };
        assert!(matches_query(
            &element,
            &UiFindQuery {
                name: Some("SAVE".to_owned()),
                ..UiFindQuery::default()
            }
        ));
        assert!(!matches_query(
            &element,
            &UiFindQuery {
                name: Some("Save".to_owned()),
                exact_name: true,
                ..UiFindQuery::default()
            }
        ));
    }
}
