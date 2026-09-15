use std::{
    collections::HashMap,
    io::{Read, Write},
    process::ExitStatus,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use command_group::GroupChild;
use latch_core::{ProcessId, Workspace};
use tracing::{info, instrument, warn};

use crate::{
    process::{spawn_grouped, terminate_group},
    CommandSpec, ExecError, ManagedOutput, ManagedStreamOutput, ProcessPoll, ProcessStart,
    ProcessState, ProcessStatus, ShutdownReport,
};

const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_STDIN_BYTES: usize = 64 * 1024;

type ProcessHandle = Arc<Mutex<ManagedProcess>>;

#[derive(Debug, Default)]
pub struct ProcessManager {
    processes: Mutex<HashMap<ProcessId, ProcessHandle>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[instrument(skip(self, workspace, spec), fields(workspace_id = %workspace.id(), program = %spec.program))]
    pub fn start(&self, workspace: &Workspace, spec: &CommandSpec) -> Result<ProcessId, ExecError> {
        self.start_detailed(workspace, spec)
            .map(|started| started.process_id)
    }

    #[instrument(skip(self, workspace, spec), fields(workspace_id = %workspace.id(), program = %spec.program))]
    pub fn start_detailed(
        &self,
        workspace: &Workspace,
        spec: &CommandSpec,
    ) -> Result<ProcessStart, ExecError> {
        let spawned = spawn_grouped(workspace, spec)?;
        let child = spawned.child;

        let stdout_buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let stderr_buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let stdout_reader = spawn_reader("stdout", spawned.stdout, Arc::clone(&stdout_buffer));
        let stderr_reader = spawn_reader("stderr", spawned.stderr, Arc::clone(&stderr_buffer));

        let process_id = ProcessId::new();
        let os_pid = child.id();
        let process = ManagedProcess {
            child,
            stdin: Some(spawned.stdin),
            started: Instant::now(),
            finished: None,
            stdout: stdout_buffer,
            stderr: stderr_buffer,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            stdout_cursor: 0,
            stderr_cursor: 0,
        };

        lock(&self.processes).insert(process_id, Arc::new(Mutex::new(process)));
        info!(%process_id, os_pid, "managed process started");
        Ok(ProcessStart {
            process_id,
            pid: os_pid,
            started_at_ms: now_ms(),
        })
    }

    pub fn status(&self, process_id: ProcessId) -> Result<ProcessStatus, ExecError> {
        let handle = self.process(process_id)?;
        let mut process = lock(&handle);
        process.refresh(process_id)?;
        Ok(process.status())
    }

    pub fn output(&self, process_id: ProcessId) -> Result<ManagedOutput, ExecError> {
        let handle = self.process(process_id)?;
        let process = lock(&handle);
        let stdout = lock(&process.stdout).snapshot();
        let stderr = lock(&process.stderr).snapshot();

        Ok(ManagedOutput { stdout, stderr })
    }

    pub fn poll(&self, process_id: ProcessId) -> Result<ProcessPoll, ExecError> {
        let handle = self.process(process_id)?;
        let mut process = lock(&handle);
        process.refresh(process_id)?;
        let (stdout, next_stdout) = lock(&process.stdout).since(process.stdout_cursor);
        let (stderr, next_stderr) = lock(&process.stderr).since(process.stderr_cursor);
        process.stdout_cursor = next_stdout;
        process.stderr_cursor = next_stderr;
        Ok(ProcessPoll {
            status: process.status(),
            stdout,
            stderr,
        })
    }

    pub fn write_stdin(
        &self,
        process_id: ProcessId,
        text: &str,
        close_stdin: bool,
    ) -> Result<(), ExecError> {
        if text.len() > MAX_STDIN_BYTES {
            return Err(ExecError::ProcessFailed {
                message: format!("stdin payload exceeds {MAX_STDIN_BYTES} bytes"),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "stdin too large"),
            });
        }
        let handle = self.process(process_id)?;
        let mut process = lock(&handle);
        process.refresh(process_id)?;
        if process.finished.is_some() {
            return Err(ExecError::ProcessFailed {
                message: format!("process {process_id} is not running"),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "process exited"),
            });
        }
        if !text.is_empty() {
            let stdin = process.stdin.as_mut().ok_or_else(|| ExecError::ProcessFailed {
                message: format!("stdin for process {process_id} is closed"),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin closed"),
            })?;
            stdin
                .write_all(text.as_bytes())
                .and_then(|()| stdin.flush())
                .map_err(|source| ExecError::ProcessFailed {
                    message: format!("could not write stdin for process {process_id}"),
                    source,
                })?;
        }
        if close_stdin {
            process.stdin.take();
        }
        Ok(())
    }

    #[instrument(skip(self), fields(%process_id))]
    pub fn kill(&self, process_id: ProcessId) -> Result<ProcessStatus, ExecError> {
        let handle = self.process(process_id)?;
        let mut process = lock(&handle);
        let terminated = process.terminate(process_id)?;
        if terminated {
            info!("managed process terminated");
        }
        Ok(process.status())
    }

    pub fn shutdown_all(&self) -> ShutdownReport {
        let handles = {
            let processes = lock(&self.processes);
            processes
                .iter()
                .map(|(process_id, process)| (*process_id, Arc::clone(process)))
                .collect::<Vec<_>>()
        };

        let mut report = ShutdownReport {
            examined: handles.len(),
            ..ShutdownReport::default()
        };

        for (process_id, handle) in handles {
            let result = lock(&handle).terminate(process_id);
            match result {
                Ok(true) => report.terminated += 1,
                Ok(false) => {}
                Err(error) => {
                    report.failures += 1;
                    warn!(%process_id, error = %error, "managed process shutdown failed");
                }
            }
        }

        if report.terminated > 0 || report.failures > 0 {
            info!(
                examined = report.examined,
                terminated = report.terminated,
                failures = report.failures,
                "managed process shutdown complete"
            );
        }

        report
    }

    fn process(&self, process_id: ProcessId) -> Result<ProcessHandle, ExecError> {
        lock(&self.processes)
            .get(&process_id)
            .cloned()
            .ok_or(ExecError::ProcessNotFound { process_id })
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[derive(Debug)]
struct ManagedProcess {
    child: GroupChild,
    stdin: Option<std::process::ChildStdin>,
    started: Instant,
    finished: Option<FinishedProcess>,
    stdout: Arc<Mutex<OutputBuffer>>,
    stderr: Arc<Mutex<OutputBuffer>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
    stdout_cursor: u64,
    stderr_cursor: u64,
}

impl ManagedProcess {
    fn refresh(&mut self, process_id: ProcessId) -> Result<(), ExecError> {
        if self.finished.is_some() {
            return Ok(());
        }

        let status = self
            .child
            .try_wait()
            .map_err(|source| ExecError::ProcessFailed {
                message: format!("could not query process {process_id}"),
                source,
            })?;

        if let Some(status) = status {
            self.stdin.take();
            self.finished = Some(FinishedProcess {
                status,
                elapsed: self.started.elapsed(),
            });
        }
        Ok(())
    }

    fn terminate(&mut self, process_id: ProcessId) -> Result<bool, ExecError> {
        self.refresh(process_id)?;
        let was_running = self.finished.is_none();

        if was_running {
            self.stdin.take();
            let status =
                terminate_group(&mut self.child, &format!("managed process {process_id}"))?;
            self.finished = Some(FinishedProcess {
                status,
                elapsed: self.started.elapsed(),
            });
        }

        self.join_readers(process_id)?;
        Ok(was_running)
    }

    fn join_readers(&mut self, process_id: ProcessId) -> Result<(), ExecError> {
        let stdout_result = join_reader(process_id, "stdout", self.stdout_reader.take());
        let stderr_result = join_reader(process_id, "stderr", self.stderr_reader.take());

        stdout_result?;
        stderr_result?;
        Ok(())
    }

    fn status(&self) -> ProcessStatus {
        match &self.finished {
            Some(finished) => ProcessStatus {
                state: ProcessState::Exited {
                    exit_code: finished.status.code(),
                },
                duration: finished.elapsed,
            },
            None => ProcessStatus {
                state: ProcessState::Running,
                duration: self.started.elapsed(),
            },
        }
    }
}

#[derive(Debug)]
struct FinishedProcess {
    status: ExitStatus,
    elapsed: std::time::Duration,
}

#[derive(Debug, Default)]
struct OutputBuffer {
    bytes: Vec<u8>,
    dropped: u64,
    truncated: bool,
    complete: bool,
}

impl OutputBuffer {
    fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= MAX_STREAM_BYTES {
            let removed = self.bytes.len() + chunk.len() - MAX_STREAM_BYTES;
            self.dropped = self
                .dropped
                .saturating_add(u64::try_from(removed).unwrap_or(u64::MAX));
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len() - MAX_STREAM_BYTES..]);
            self.truncated = true;
            return;
        }

        let required = self.bytes.len() + chunk.len();
        if required > MAX_STREAM_BYTES {
            let remove = required - MAX_STREAM_BYTES;
            self.bytes.drain(..remove);
            self.dropped = self
                .dropped
                .saturating_add(u64::try_from(remove).unwrap_or(u64::MAX));
            self.truncated = true;
        }
        self.bytes.extend_from_slice(chunk);
    }

    fn snapshot(&self) -> ManagedStreamOutput {
        ManagedStreamOutput {
            text: String::from_utf8_lossy(&self.bytes).into_owned(),
            truncated: self.truncated,
            complete: self.complete,
        }
    }

    fn since(&self, cursor: u64) -> (ManagedStreamOutput, u64) {
        let end = self
            .dropped
            .saturating_add(u64::try_from(self.bytes.len()).unwrap_or(u64::MAX));
        let effective = cursor.max(self.dropped).min(end);
        let offset = usize::try_from(effective.saturating_sub(self.dropped))
            .unwrap_or(self.bytes.len());
        (
            ManagedStreamOutput {
                text: String::from_utf8_lossy(&self.bytes[offset..]).into_owned(),
                truncated: cursor < self.dropped,
                complete: self.complete,
            },
            end,
        )
    }
}

fn spawn_reader(
    stream_name: &'static str,
    stream: impl Read + Send + 'static,
    buffer: Arc<Mutex<OutputBuffer>>,
) -> JoinHandle<()> {
    thread::spawn(move || read_stream(stream_name, stream, &buffer))
}

fn read_stream(stream_name: &'static str, mut stream: impl Read, buffer: &Mutex<OutputBuffer>) {
    let mut chunk = [0_u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                lock(buffer).complete = true;
                return;
            }
            Ok(count) => lock(buffer).append(&chunk[..count]),
            Err(error) => {
                warn!(stream = stream_name, error = %error, "managed process output read failed");
                return;
            }
        }
    }
}

fn join_reader(
    process_id: ProcessId,
    stream_name: &'static str,
    reader: Option<JoinHandle<()>>,
) -> Result<(), ExecError> {
    let Some(reader) = reader else {
        return Ok(());
    };

    reader.join().map_err(|_| ExecError::ProcessFailed {
        message: format!("{stream_name} reader for process {process_id} panicked"),
        source: std::io::Error::other("managed output reader thread panicked"),
    })
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
