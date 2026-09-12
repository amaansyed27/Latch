use std::{
    collections::HashMap,
    io::Read,
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::Instant,
};

use latch_core::{ProcessId, Workspace};
use tracing::{info, instrument};

use crate::{
    CommandSpec, ExecError, ManagedOutput, ManagedStreamOutput, ProcessState, ProcessStatus,
};

const MAX_STREAM_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct ProcessManager {
    processes: Mutex<HashMap<ProcessId, ManagedProcess>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[instrument(skip(self, workspace, spec), fields(workspace_id = %workspace.id(), program = %spec.program))]
    pub fn start(&self, workspace: &Workspace, spec: &CommandSpec) -> Result<ProcessId, ExecError> {
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(workspace.root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ExecError::spawn(&spec.program, source))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ExecError::ProcessFailed {
                message: "stdout pipe was not available".to_owned(),
                source: std::io::Error::other("stdout pipe missing after spawn"),
            })?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ExecError::ProcessFailed {
                message: "stderr pipe was not available".to_owned(),
                source: std::io::Error::other("stderr pipe missing after spawn"),
            })?;

        let stdout_buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let stderr_buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        spawn_reader(stdout, Arc::clone(&stdout_buffer));
        spawn_reader(stderr, Arc::clone(&stderr_buffer));

        let process_id = ProcessId::new();
        let os_pid = child.id();
        let process = ManagedProcess {
            child,
            started: Instant::now(),
            finished: None,
            stdout: stdout_buffer,
            stderr: stderr_buffer,
        };

        lock(&self.processes).insert(process_id, process);
        info!(%process_id, os_pid, "managed process started");
        Ok(process_id)
    }

    pub fn status(&self, process_id: ProcessId) -> Result<ProcessStatus, ExecError> {
        let mut processes = lock(&self.processes);
        let process = processes
            .get_mut(&process_id)
            .ok_or(ExecError::ProcessNotFound { process_id })?;
        process.refresh(process_id)?;
        Ok(process.status())
    }

    pub fn output(&self, process_id: ProcessId) -> Result<ManagedOutput, ExecError> {
        let processes = lock(&self.processes);
        let process = processes
            .get(&process_id)
            .ok_or(ExecError::ProcessNotFound { process_id })?;
        let stdout = lock(&process.stdout).snapshot();
        let stderr = lock(&process.stderr).snapshot();

        Ok(ManagedOutput { stdout, stderr })
    }

    #[instrument(skip(self), fields(%process_id))]
    pub fn kill(&self, process_id: ProcessId) -> Result<ProcessStatus, ExecError> {
        let mut processes = lock(&self.processes);
        let process = processes
            .get_mut(&process_id)
            .ok_or(ExecError::ProcessNotFound { process_id })?;

        process.refresh(process_id)?;
        if process.finished.is_none() {
            process
                .child
                .kill()
                .map_err(|source| ExecError::ProcessFailed {
                    message: format!("could not terminate process {process_id}"),
                    source,
                })?;
            let status = process
                .child
                .wait()
                .map_err(|source| ExecError::ProcessFailed {
                    message: format!("could not wait for process {process_id} after termination"),
                    source,
                })?;
            process.finished = Some(FinishedProcess {
                status,
                elapsed: process.started.elapsed(),
            });
            info!("managed process terminated");
        }

        Ok(process.status())
    }
}

#[derive(Debug)]
struct ManagedProcess {
    child: Child,
    started: Instant,
    finished: Option<FinishedProcess>,
    stdout: Arc<Mutex<OutputBuffer>>,
    stderr: Arc<Mutex<OutputBuffer>>,
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
            self.finished = Some(FinishedProcess {
                status,
                elapsed: self.started.elapsed(),
            });
        }
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
    truncated: bool,
    complete: bool,
}

impl OutputBuffer {
    fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= MAX_STREAM_BYTES {
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
}

trait ManagedStream: Read + Send + 'static {}
impl ManagedStream for ChildStdout {}
impl ManagedStream for ChildStderr {}

fn spawn_reader(stream: impl ManagedStream, buffer: Arc<Mutex<OutputBuffer>>) {
    thread::spawn(move || read_stream(stream, &buffer));
}

fn read_stream(mut stream: impl Read, buffer: &Mutex<OutputBuffer>) {
    let mut chunk = [0_u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => lock(buffer).append(&chunk[..count]),
        }
    }
    lock(buffer).complete = true;
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
