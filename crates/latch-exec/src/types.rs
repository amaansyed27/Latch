use std::time::Duration;

pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const DEFAULT_RUN_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub timeout: Option<Duration>,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_RUN_TIMEOUT),
            max_stdout_bytes: DEFAULT_RUN_OUTPUT_BYTES,
            max_stderr_bytes: DEFAULT_RUN_OUTPUT_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Exited { exit_code: Option<i32> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStatus {
    pub state: ProcessState,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedStreamOutput {
    pub text: String,
    pub truncated: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedOutput {
    pub stdout: ManagedStreamOutput,
    pub stderr: ManagedStreamOutput,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    pub examined: usize,
    pub terminated: usize,
    pub failures: usize,
}
