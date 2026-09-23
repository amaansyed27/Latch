use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, MutexGuard, PoisonError},
};

use latch_core::{BrowserContextId, TabId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STRING_CHARS: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfile {
    Isolated,
    Persistent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserContextInfo {
    pub context_id: BrowserContextId,
    pub profile: BrowserProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTabInfo {
    pub tab_id: TabId,
    pub context_id: BrowserContextId,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BrowserTarget {
    pub element_ref: Option<String>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub label: Option<String>,
    pub test_id: Option<String>,
    pub css: Option<String>,
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserElement {
    pub element_ref: String,
    pub role: Option<String>,
    pub name: String,
    pub tag: String,
    pub visible: bool,
    pub enabled: bool,
    pub bounds: Option<BrowserBounds>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct BrowserBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserAction {
    Click,
    Fill { value: String },
    Press { key: String },
    Check,
    Uncheck,
    SelectOption { value: String },
    Hover,
    Focus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BrowserVerification {
    pub url_contains: Option<String>,
    pub text_present: Option<String>,
    pub selector_visible: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserActionResult {
    pub url: String,
    pub title: String,
    pub verification: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshot {
    pub url: String,
    pub title: String,
    pub aria: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserScreenshot {
    pub mime_type: String,
    pub data_base64: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserLogEntry {
    pub sequence: u64,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("Playwright browser provider is unavailable: {0}")]
    Unavailable(String),
    #[error("browser provider protocol failed: {0}")]
    Protocol(String),
    #[error("browser provider operation failed: {0}")]
    Operation(String),
    #[error("browser input is invalid: {0}")]
    InvalidInput(String),
    #[error("browser I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct BrowserManager {
    process: Mutex<Option<BrowserProcess>>,
    bridge_path: PathBuf,
    profile_root: PathBuf,
}

impl BrowserManager {
    pub fn new(profile_root: impl Into<PathBuf>) -> Self {
        Self {
            process: Mutex::new(None),
            bridge_path: default_bridge_path(),
            profile_root: profile_root.into(),
        }
    }

    pub fn with_bridge(profile_root: impl Into<PathBuf>, bridge_path: impl Into<PathBuf>) -> Self {
        Self {
            process: Mutex::new(None),
            bridge_path: bridge_path.into(),
            profile_root: profile_root.into(),
        }
    }

    pub fn status(&self) -> Result<Value, BrowserError> {
        self.call("status", json!({}))
    }

    pub fn create_context(
        &self,
        profile: BrowserProfile,
    ) -> Result<BrowserContextInfo, BrowserError> {
        let context_id = BrowserContextId::new();
        let profile_dir = match profile {
            BrowserProfile::Isolated => None,
            BrowserProfile::Persistent => Some(
                self.profile_root
                    .join("browser-profile")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        self.call(
            "context.create",
            json!({
                "context_id": context_id,
                "profile": profile,
                "profile_dir": profile_dir,
            }),
        )?;
        Ok(BrowserContextInfo {
            context_id,
            profile,
        })
    }

    pub fn list_contexts(&self) -> Result<Vec<BrowserContextInfo>, BrowserError> {
        from_value(self.call("context.list", json!({}))?)
    }

    pub fn close_context(&self, context_id: BrowserContextId) -> Result<(), BrowserError> {
        self.call("context.close", json!({ "context_id": context_id }))?;
        Ok(())
    }

    pub fn new_tab(
        &self,
        context_id: BrowserContextId,
        url: Option<&str>,
    ) -> Result<BrowserTabInfo, BrowserError> {
        if let Some(url) = url {
            validate_string(url, MAX_STRING_CHARS, "URL")?;
        }
        let tab_id = TabId::new();
        from_value(self.call(
            "tab.create",
            json!({
                "context_id": context_id,
                "tab_id": tab_id,
                "url": url,
            }),
        )?)
    }

    pub fn list_tabs(
        &self,
        context_id: Option<BrowserContextId>,
    ) -> Result<Vec<BrowserTabInfo>, BrowserError> {
        from_value(self.call("tab.list", json!({ "context_id": context_id }))?)
    }

    pub fn close_tab(&self, tab_id: TabId) -> Result<(), BrowserError> {
        self.call("tab.close", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    pub fn navigate(&self, tab_id: TabId, url: &str) -> Result<BrowserTabInfo, BrowserError> {
        validate_string(url, MAX_STRING_CHARS, "URL")?;
        from_value(self.call("navigate", json!({ "tab_id": tab_id, "url": url }))?)
    }

    pub fn snapshot(&self, tab_id: TabId) -> Result<BrowserSnapshot, BrowserError> {
        from_value(self.call("snapshot", json!({ "tab_id": tab_id }))?)
    }

    pub fn find(
        &self,
        tab_id: TabId,
        target: BrowserTarget,
        max_results: usize,
    ) -> Result<Vec<BrowserElement>, BrowserError> {
        validate_target(&target)?;
        from_value(self.call(
            "find",
            json!({
                "tab_id": tab_id,
                "target": target,
                "max_results": max_results.clamp(1, 25),
            }),
        )?)
    }

    pub fn act(
        &self,
        tab_id: TabId,
        target: BrowserTarget,
        action: BrowserAction,
        verification: Option<BrowserVerification>,
    ) -> Result<BrowserActionResult, BrowserError> {
        validate_target(&target)?;
        if let BrowserAction::Fill { value } = &action {
            validate_string(value, MAX_STRING_CHARS, "fill value")?;
        }
        from_value(self.call(
            "act",
            json!({
                "tab_id": tab_id,
                "target": target,
                "action": action,
                "verification": verification,
            }),
        )?)
    }

    pub fn console(
        &self,
        tab_id: TabId,
        after_sequence: u64,
        max_entries: usize,
    ) -> Result<Vec<BrowserLogEntry>, BrowserError> {
        self.log_entries("console", tab_id, after_sequence, max_entries)
    }

    pub fn network(
        &self,
        tab_id: TabId,
        after_sequence: u64,
        max_entries: usize,
    ) -> Result<Vec<BrowserLogEntry>, BrowserError> {
        self.log_entries("network", tab_id, after_sequence, max_entries)
    }

    pub fn downloads(
        &self,
        tab_id: TabId,
        after_sequence: u64,
        max_entries: usize,
    ) -> Result<Vec<BrowserLogEntry>, BrowserError> {
        self.log_entries("downloads", tab_id, after_sequence, max_entries)
    }

    pub fn screenshot(&self, tab_id: TabId) -> Result<BrowserScreenshot, BrowserError> {
        from_value(self.call("screenshot", json!({ "tab_id": tab_id }))?)
    }

    pub fn page_state(&self, tab_id: TabId) -> Result<BrowserTabInfo, BrowserError> {
        from_value(self.call("page.state", json!({ "tab_id": tab_id }))?)
    }

    pub fn shutdown(&self) {
        let mut guard = lock(&self.process);
        if let Some(mut process) = guard.take() {
            let _ = process.call("shutdown", json!({}));
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }

    fn log_entries(
        &self,
        operation: &str,
        tab_id: TabId,
        after_sequence: u64,
        max_entries: usize,
    ) -> Result<Vec<BrowserLogEntry>, BrowserError> {
        from_value(self.call(
            operation,
            json!({
                "tab_id": tab_id,
                "after_sequence": after_sequence,
                "max_entries": max_entries.clamp(1, 100),
            }),
        )?)
    }

    fn call(&self, operation: &str, params: Value) -> Result<Value, BrowserError> {
        let mut guard = lock(&self.process);
        if guard.is_none() {
            *guard = Some(BrowserProcess::spawn(&self.bridge_path)?);
        }
        let result = guard
            .as_mut()
            .expect("browser process initialized")
            .call(operation, params);
        if matches!(result, Err(BrowserError::Io(_) | BrowserError::Protocol(_))) {
            if let Some(mut process) = guard.take() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
        result
    }
}

impl Drop for BrowserManager {
    fn drop(&mut self) {
        if let Ok(process) = self.process.get_mut() {
            if let Some(mut process) = process.take() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }
}

struct BrowserProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl BrowserProcess {
    fn spawn(bridge_path: &Path) -> Result<Self, BrowserError> {
        if !bridge_path.is_file() {
            return Err(BrowserError::Unavailable(format!(
                "bridge script is missing: {}",
                bridge_path.display()
            )));
        }
        let node = env::var_os("LATCH_NODE_PATH").unwrap_or_else(|| "node".into());
        let mut command = Command::new(node);
        command
            .arg(bridge_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|error| BrowserError::Unavailable(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BrowserError::Unavailable("browser stdin unavailable".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BrowserError::Unavailable("browser stdout unavailable".to_owned()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn call(&mut self, operation: &str, params: Value) -> Result<Value, BrowserError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = json!({ "id": id, "operation": operation, "params": params });
        serde_json::to_writer(&mut self.stdin, &request)
            .map_err(|error| BrowserError::Protocol(error.to_string()))?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(BrowserError::Protocol(
                "browser provider closed unexpectedly".to_owned(),
            ));
        }
        if line.len() > MAX_RESPONSE_BYTES {
            return Err(BrowserError::Protocol(
                "browser provider response exceeded the local limit".to_owned(),
            ));
        }
        let response: ProviderResponse = serde_json::from_str(&line)
            .map_err(|error| BrowserError::Protocol(error.to_string()))?;
        if response.id != id {
            return Err(BrowserError::Protocol(
                "browser provider response ID mismatch".to_owned(),
            ));
        }
        match response.error {
            Some(error) => Err(BrowserError::Operation(error)),
            None => Ok(response.result.unwrap_or(Value::Null)),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProviderResponse {
    id: u64,
    result: Option<Value>,
    error: Option<String>,
}

fn from_value<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, BrowserError> {
    serde_json::from_value(value).map_err(|error| BrowserError::Protocol(error.to_string()))
}

fn validate_string(value: &str, max_chars: usize, label: &str) -> Result<(), BrowserError> {
    if value.chars().count() > max_chars {
        Err(BrowserError::InvalidInput(format!(
            "{label} exceeds {max_chars} characters"
        )))
    } else {
        Ok(())
    }
}

fn validate_target(target: &BrowserTarget) -> Result<(), BrowserError> {
    let count = [
        target.element_ref.is_some(),
        target.role.is_some(),
        target.text.is_some(),
        target.label.is_some(),
        target.test_id.is_some(),
        target.css.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count();
    if count != 1 {
        return Err(BrowserError::InvalidInput(
            "browser target must specify exactly one locator kind".to_owned(),
        ));
    }
    for value in [
        target.element_ref.as_deref(),
        target.role.as_deref(),
        target.name.as_deref(),
        target.text.as_deref(),
        target.label.as_deref(),
        target.test_id.as_deref(),
        target.css.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        validate_string(value, 4096, "browser locator")?;
    }
    Ok(())
}

fn default_bridge_path() -> PathBuf {
    if let Some(path) = env::var_os("LATCH_BROWSER_BRIDGE") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(directory) = exe.parent() {
            let installed = directory.join("browser-runtime").join("bridge.mjs");
            if installed.is_file() {
                return installed;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("runtime")
        .join("browser")
        .join("bridge.mjs")
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_bounded_and_unambiguous() {
        assert!(validate_target(&BrowserTarget {
            role: Some("button".to_owned()),
            name: Some("Save".to_owned()),
            ..BrowserTarget::default()
        })
        .is_ok());
        assert!(validate_target(&BrowserTarget {
            role: Some("button".to_owned()),
            css: Some("button".to_owned()),
            ..BrowserTarget::default()
        })
        .is_err());
    }

    #[test]
    fn provider_is_lazy() {
        let manager = BrowserManager::with_bridge(
            std::env::temp_dir(),
            std::env::temp_dir().join("missing-latch-browser-bridge.mjs"),
        );
        assert!(manager.list_contexts().is_err());
    }
}
