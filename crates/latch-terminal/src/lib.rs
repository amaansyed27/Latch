#[cfg(not(windows))]
use std::env;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    thread::{self, JoinHandle},
};
#[cfg(windows)]
use std::{path::PathBuf, process::Command};

use latch_core::{
    resolver::{resolve_route, ProviderRoute, RouteCandidate, RouteIntent},
    runtime_events::{self, ResourceKey},
    TerminalId,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const DEFAULT_SNAPSHOT_BYTES: usize = 64 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;
const MAX_WRITE_BYTES: usize = 64 * 1024;
const MAX_LOGICAL_LINES: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShellProfile {
    pub id: String,
    pub display_name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalCreated {
    pub terminal_id: TerminalId,
    pub profile: ShellProfile,
    pub pid: Option<u32>,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TerminalState {
    Running,
    Exited { exit_code: u32 },
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub terminal_id: TerminalId,
    pub state: TerminalState,
    pub sequence: u64,
    pub output: String,
    pub logical_screen: String,
    pub truncated: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalInfo {
    pub terminal_id: TerminalId,
    pub profile_id: String,
    pub pid: Option<u32>,
    pub state: TerminalState,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal profile was not found: {0}")]
    ProfileNotFound(String),
    #[error("terminal was not found: {0}")]
    NotFound(TerminalId),
    #[error("terminal is no longer running: {0}")]
    NotRunning(TerminalId),
    #[error("terminal payload exceeds {MAX_WRITE_BYTES} bytes")]
    PayloadTooLarge,
    #[error("could not create terminal: {0}")]
    Create(String),
    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

type TerminalHandle = Arc<Mutex<ManagedTerminal>>;

pub struct TerminalManager {
    terminals: Mutex<HashMap<TerminalId, TerminalHandle>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
        }
    }

    pub fn profiles(&self) -> Vec<ShellProfile> {
        discover_shell_profiles()
    }

    pub fn create(
        &self,
        profile_id: Option<&str>,
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<TerminalCreated, TerminalError> {
        let resolution = resolve_route(
            RouteIntent::InteractiveTerminal,
            &[RouteCandidate::new(
                ProviderRoute::ConPty,
                true,
                true,
                100,
                true,
            )],
        )
        .map_err(|error| TerminalError::Create(format!("terminal route unavailable: {error:?}")))?;
        debug_assert_eq!(resolution.route, ProviderRoute::ConPty);

        let profiles = discover_shell_profiles();
        let profile = match profile_id {
            Some(id) => profiles
                .into_iter()
                .find(|profile| profile.id == id)
                .ok_or_else(|| TerminalError::ProfileNotFound(id.to_owned()))?,
            None => profiles
                .into_iter()
                .next()
                .ok_or_else(|| TerminalError::ProfileNotFound("default".to_owned()))?,
        };

        let rows = rows.clamp(2, 300);
        let cols = cols.clamp(10, 500);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalError::Create(error.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalError::Create(error.to_string()))?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|error| TerminalError::Create(error.to_string()))?,
        ));

        let mut command = CommandBuilder::new(&profile.program);
        command.args(&profile.args);
        command.cwd(cwd.as_os_str());
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalError::Create(error.to_string()))?;
        drop(pair.slave);

        #[cfg(windows)]
        let job = attach_windows_job(child.as_ref())?;

        let terminal_id = TerminalId::new();
        let output = Arc::new(Mutex::new(OutputRing::default()));
        let reader_task = spawn_reader(
            reader,
            Arc::clone(&output),
            Arc::downgrade(&writer),
            terminal_id,
        );
        let pid = child.process_id();
        let terminal = ManagedTerminal {
            profile: profile.clone(),
            master: Some(pair.master),
            child,
            writer: Some(writer),
            output,
            reader_task: Some(reader_task),
            killed: false,
            rows,
            cols,
            #[cfg(windows)]
            job: Some(job),
        };
        lock(&self.terminals).insert(terminal_id, Arc::new(Mutex::new(terminal)));
        info!(%terminal_id, ?pid, profile = %profile.id, "persistent terminal created");
        Ok(TerminalCreated {
            terminal_id,
            profile,
            pid,
            rows,
            cols,
        })
    }

    pub fn write(&self, terminal_id: TerminalId, text: &str) -> Result<(), TerminalError> {
        if text.len() > MAX_WRITE_BYTES {
            return Err(TerminalError::PayloadTooLarge);
        }
        let handle = self.terminal(terminal_id)?;
        let mut terminal = lock(&handle);
        terminal.ensure_running(terminal_id)?;
        let writer = terminal
            .writer
            .as_ref()
            .ok_or(TerminalError::NotRunning(terminal_id))?;
        let mut writer = lock(writer);
        writer.write_all(text.as_bytes())?;
        writer.flush()?;
        Ok(())
    }

    pub fn interrupt(&self, terminal_id: TerminalId) -> Result<(), TerminalError> {
        self.write(terminal_id, "\u{3}")
    }

    pub fn resize(
        &self,
        terminal_id: TerminalId,
        rows: u16,
        cols: u16,
    ) -> Result<(), TerminalError> {
        let handle = self.terminal(terminal_id)?;
        let mut terminal = lock(&handle);
        terminal.ensure_running(terminal_id)?;
        terminal
            .master
            .as_ref()
            .ok_or(TerminalError::NotRunning(terminal_id))?
            .resize(PtySize {
                rows: rows.clamp(2, 300),
                cols: cols.clamp(10, 500),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalError::Create(error.to_string()))?;
        terminal.rows = rows.clamp(2, 300);
        terminal.cols = cols.clamp(10, 500);
        Ok(())
    }

    pub fn snapshot(
        &self,
        terminal_id: TerminalId,
        after_sequence: Option<u64>,
        max_bytes: Option<usize>,
    ) -> Result<TerminalSnapshot, TerminalError> {
        let handle = self.terminal(terminal_id)?;
        let mut terminal = lock(&handle);
        let state = terminal.state()?;
        let output = lock(&terminal.output).snapshot(
            after_sequence.unwrap_or(0),
            max_bytes
                .unwrap_or(DEFAULT_SNAPSHOT_BYTES)
                .clamp(1, MAX_SNAPSHOT_BYTES),
        );
        Ok(TerminalSnapshot {
            terminal_id,
            state,
            sequence: output.sequence,
            logical_screen: logical_screen(&output.text),
            output: output.text,
            truncated: output.truncated,
            complete: output.complete,
        })
    }

    pub fn list(&self) -> Result<Vec<TerminalInfo>, TerminalError> {
        let handles = lock(&self.terminals)
            .iter()
            .map(|(id, handle)| (*id, Arc::clone(handle)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|(terminal_id, handle)| {
                let mut terminal = lock(&handle);
                Ok(TerminalInfo {
                    terminal_id,
                    profile_id: terminal.profile.id.clone(),
                    pid: terminal.child.process_id(),
                    state: terminal.state()?,
                    rows: terminal.rows,
                    cols: terminal.cols,
                })
            })
            .collect()
    }

    pub fn kill(&self, terminal_id: TerminalId) -> Result<TerminalState, TerminalError> {
        let handle = self.terminal(terminal_id)?;
        let mut terminal = lock(&handle);
        terminal.kill(terminal_id)?;
        Ok(TerminalState::Killed)
    }

    pub fn remove(&self, terminal_id: TerminalId) -> Result<(), TerminalError> {
        let handle = lock(&self.terminals)
            .remove(&terminal_id)
            .ok_or(TerminalError::NotFound(terminal_id))?;
        let _ = lock(&handle).kill(terminal_id);
        runtime_events::unbind_resource(ResourceKey::Terminal(terminal_id));
        Ok(())
    }

    pub fn shutdown_all(&self) {
        let handles = lock(&self.terminals)
            .drain()
            .collect::<Vec<_>>();
        for (terminal_id, handle) in handles {
            if let Err(error) = lock(&handle).kill(terminal_id) {
                warn!(%error, "terminal shutdown failed");
            }
            runtime_events::unbind_resource(ResourceKey::Terminal(terminal_id));
        }
    }

    fn terminal(&self, terminal_id: TerminalId) -> Result<TerminalHandle, TerminalError> {
        lock(&self.terminals)
            .get(&terminal_id)
            .cloned()
            .ok_or(TerminalError::NotFound(terminal_id))
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

struct ManagedTerminal {
    profile: ShellProfile,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    output: Arc<Mutex<OutputRing>>,
    reader_task: Option<JoinHandle<()>>,
    killed: bool,
    rows: u16,
    cols: u16,
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

impl ManagedTerminal {
    fn state(&mut self) -> Result<TerminalState, TerminalError> {
        if self.killed {
            return Ok(TerminalState::Killed);
        }
        match self.child.try_wait()? {
            Some(status) => Ok(TerminalState::Exited {
                exit_code: status.exit_code(),
            }),
            None => Ok(TerminalState::Running),
        }
    }

    fn ensure_running(&mut self, terminal_id: TerminalId) -> Result<(), TerminalError> {
        if matches!(self.state()?, TerminalState::Running) {
            Ok(())
        } else {
            Err(TerminalError::NotRunning(terminal_id))
        }
    }

    fn kill(&mut self, terminal_id: TerminalId) -> Result<(), TerminalError> {
        if matches!(self.state()?, TerminalState::Running) {
            self.child.kill()?;
            #[cfg(windows)]
            self.job.take();
            let _ = self.child.wait()?;
            runtime_events::publish_resource(
                ResourceKey::Terminal(terminal_id),
                "terminal",
                "process.exited",
                "Terminal process exited",
                None,
            );
        }
        self.killed = true;
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader_task.take() {
            let _ = reader.join();
        }
        Ok(())
    }
}

#[derive(Default)]
struct OutputRing {
    bytes: Vec<u8>,
    dropped: u64,
    complete: bool,
}

struct OutputSlice {
    text: String,
    sequence: u64,
    truncated: bool,
    complete: bool,
}

impl OutputRing {
    fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= MAX_OUTPUT_BYTES {
            let removed = self.bytes.len() + chunk.len() - MAX_OUTPUT_BYTES;
            self.dropped = self
                .dropped
                .saturating_add(u64::try_from(removed).unwrap_or(u64::MAX));
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len() - MAX_OUTPUT_BYTES..]);
            return;
        }
        let required = self.bytes.len() + chunk.len();
        if required > MAX_OUTPUT_BYTES {
            let remove = required - MAX_OUTPUT_BYTES;
            self.bytes.drain(..remove);
            self.dropped = self
                .dropped
                .saturating_add(u64::try_from(remove).unwrap_or(u64::MAX));
        }
        self.bytes.extend_from_slice(chunk);
    }

    fn snapshot(&self, cursor: u64, max_bytes: usize) -> OutputSlice {
        let end = self
            .dropped
            .saturating_add(u64::try_from(self.bytes.len()).unwrap_or(u64::MAX));
        let effective = cursor.max(self.dropped).min(end);
        let mut offset =
            usize::try_from(effective.saturating_sub(self.dropped)).unwrap_or(self.bytes.len());
        let available = self.bytes.len().saturating_sub(offset);
        let bounded = available.min(max_bytes);
        if available > max_bytes {
            offset = self.bytes.len().saturating_sub(max_bytes);
        }
        let truncated = cursor < self.dropped || available > bounded;
        OutputSlice {
            text: String::from_utf8_lossy(&self.bytes[offset..]).into_owned(),
            sequence: end,
            truncated,
            complete: self.complete,
        }
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<OutputRing>>,
    writer: Weak<Mutex<Box<dyn Write + Send>>>,
    terminal_id: TerminalId,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        let mut query_tail = Vec::with_capacity(3);
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    lock(&output).complete = true;
                    runtime_events::publish_resource(
                        ResourceKey::Terminal(terminal_id),
                        "terminal",
                        "process.exited",
                        "Terminal process exited",
                        None,
                    );
                    return;
                }
                Ok(count) => {
                    let bytes = &chunk[..count];
                    let mut query_scan = Vec::with_capacity(query_tail.len() + bytes.len());
                    query_scan.extend_from_slice(&query_tail);
                    query_scan.extend_from_slice(bytes);
                    if query_scan.windows(4).any(|window| window == b"\x1b[6n") {
                        if let Some(writer) = writer.upgrade() {
                            let mut writer = lock(&writer);
                            if let Err(error) =
                                writer.write_all(b"\x1b[1;1R").and_then(|()| writer.flush())
                            {
                                warn!(%error, "terminal cursor-position response failed");
                            }
                        }
                    }
                    query_tail.clear();
                    let keep = query_scan.len().min(3);
                    query_tail.extend_from_slice(&query_scan[query_scan.len() - keep..]);
                    lock(&output).append(bytes);
                    runtime_events::publish_resource(
                        ResourceKey::Terminal(terminal_id),
                        "terminal",
                        "terminal.output",
                        "Terminal produced output",
                        None,
                    );
                }
                Err(error) => {
                    warn!(%error, "terminal output reader failed");
                    lock(&output).complete = true;
                    return;
                }
            }
        }
    })
}

fn logical_screen(raw: &str) -> String {
    let plain = strip_ansi(raw).replace('\r', "");
    let mut lines = plain.lines().collect::<Vec<_>>();
    if lines.len() > MAX_LOGICAL_LINES {
        lines.drain(..lines.len() - MAX_LOGICAL_LINES);
    }
    let joined = lines.join("\n");
    if joined.len() <= DEFAULT_SNAPSHOT_BYTES {
        joined
    } else {
        joined[joined.len() - DEFAULT_SNAPSHOT_BYTES..].to_owned()
    }
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek().copied() == Some('\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    output
}

pub fn discover_shell_profiles() -> Vec<ShellProfile> {
    #[cfg(windows)]
    {
        discover_windows_shell_profiles()
    }
    #[cfg(not(windows))]
    {
        let program = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        vec![ShellProfile {
            id: "shell".to_owned(),
            display_name: "Shell".to_owned(),
            program,
            args: Vec::new(),
        }]
    }
}

#[cfg(windows)]
fn discover_windows_shell_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();
    for (id, name, program) in [
        ("pwsh", "PowerShell 7", "pwsh.exe"),
        ("powershell", "Windows PowerShell", "powershell.exe"),
        ("cmd", "Command Prompt", "cmd.exe"),
    ] {
        if command_exists(program) {
            profiles.push(ShellProfile {
                id: id.to_owned(),
                display_name: name.to_owned(),
                program: program.to_owned(),
                args: Vec::new(),
            });
        }
    }
    if let Some(git_bash) = find_git_bash() {
        profiles.push(ShellProfile {
            id: "git-bash".to_owned(),
            display_name: "Git Bash".to_owned(),
            program: git_bash.to_string_lossy().into_owned(),
            args: vec!["--login".to_owned(), "-i".to_owned()],
        });
    }
    if command_exists("wsl.exe") {
        if let Ok(output) = Command::new("wsl.exe").args(["-l", "-q"]).output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).replace('\0', "");
                for distro in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                    profiles.push(ShellProfile {
                        id: format!("wsl:{distro}"),
                        display_name: format!("WSL · {distro}"),
                        program: "wsl.exe".to_owned(),
                        args: vec!["-d".to_owned(), distro.to_owned()],
                    });
                }
            }
        }
    }
    if profiles.is_empty() {
        profiles.push(ShellProfile {
            id: "cmd".to_owned(),
            display_name: "Command Prompt".to_owned(),
            program: "cmd.exe".to_owned(),
            args: Vec::new(),
        });
    }
    profiles
}

#[cfg(windows)]
fn command_exists(program: &str) -> bool {
    Command::new("where.exe")
        .arg(program)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
fn find_git_bash() -> Option<PathBuf> {
    [
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[cfg(windows)]
fn attach_windows_job(
    child: &(dyn portable_pty::Child + Send + Sync),
) -> Result<win32job::Job, TerminalError> {
    use win32job::Job;

    let raw = child.as_raw_handle().ok_or_else(|| {
        TerminalError::Create("ConPTY child process handle is unavailable".to_owned())
    })?;
    let job = Job::create().map_err(|error| TerminalError::Create(error.to_string()))?;
    let mut limits = job
        .query_extended_limit_info()
        .map_err(|error| TerminalError::Create(error.to_string()))?;
    limits.limit_kill_on_job_close();
    job.set_extended_limit_info(&limits)
        .map_err(|error| TerminalError::Create(error.to_string()))?;
    job.assign_process(raw as isize)
        .map_err(|error| TerminalError::Create(error.to_string()))?;
    Ok(job)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    use latch_core::{runtime_events, SessionId};

    use super::*;

    #[test]
    fn shell_discovery_always_has_a_default() {
        assert!(!discover_shell_profiles().is_empty());
    }

    #[test]
    fn persistent_terminal_keeps_state_and_incremental_output() {
        let manager = TerminalManager::new();
        let cwd = std::env::current_dir().unwrap();
        let created = manager.create(None, &cwd, 24, 100).unwrap();
        manager
            .write(created.terminal_id, "echo LATCH_PTY_TEST\r\n")
            .unwrap();
        let first_deadline = Instant::now() + Duration::from_secs(5);
        let first = loop {
            let snapshot = manager
                .snapshot(created.terminal_id, None, Some(32 * 1024))
                .unwrap();
            if snapshot.logical_screen.contains("LATCH_PTY_TEST") {
                break snapshot;
            }
            assert!(
                Instant::now() < first_deadline,
                "timed out waiting for first terminal output: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        manager
            .write(created.terminal_id, "echo SECOND\r\n")
            .unwrap();
        let second_deadline = Instant::now() + Duration::from_secs(5);
        let second = loop {
            let snapshot = manager
                .snapshot(created.terminal_id, Some(first.sequence), Some(32 * 1024))
                .unwrap();
            if snapshot.output.contains("SECOND") {
                break snapshot;
            }
            assert!(
                Instant::now() < second_deadline,
                "timed out waiting for incremental terminal output: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        assert!(second.output.contains("SECOND"));
        manager.kill(created.terminal_id).unwrap();
    }

    #[test]
    fn terminal_output_wakes_event_wait_without_terminal_read() {
        let manager = TerminalManager::new();
        let cwd = std::env::current_dir().unwrap();
        let created = manager.create(None, &cwd, 24, 100).unwrap();
        let session = SessionId::new();
        runtime_events::bind_resource(ResourceKey::Terminal(created.terminal_id), session);
        let cursor = runtime_events::latest_sequence();
        manager
            .write(created.terminal_id, "echo LATCH_ASYNC_EVENT\r\n")
            .unwrap();
        let events = runtime_events::read(
            session,
            cursor,
            &["terminal.output".to_owned()],
            5_000,
            10,
        );
        assert!(!events.is_empty());
        manager.remove(created.terminal_id).unwrap();
        runtime_events::clear_session(session);
    }

    #[test]
    fn ansi_is_removed_from_logical_screen() {
        assert_eq!(logical_screen("a\u{1b}[31mb\u{1b}[0m\r\nc"), "ab\nc");
    }
}
