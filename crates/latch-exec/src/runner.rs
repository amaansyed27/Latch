use std::{
    io::Read,
    process::ExitStatus,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use command_group::GroupChild;
use latch_core::Workspace;
use tracing::{info, instrument, warn};

use crate::{
    process::{spawn_grouped, terminate_group},
    CommandSpec, ExecError, ExecutionResult, RunOptions,
};

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[instrument(skip(workspace, spec), fields(workspace_id = %workspace.id(), program = %spec.program))]
pub fn run_blocking(
    workspace: &Workspace,
    spec: &CommandSpec,
) -> Result<ExecutionResult, ExecError> {
    run_blocking_with_options(workspace, spec, &RunOptions::default())
}

#[instrument(skip(workspace, spec, options), fields(workspace_id = %workspace.id(), program = %spec.program))]
pub fn run_blocking_with_options(
    workspace: &Workspace,
    spec: &CommandSpec,
    options: &RunOptions,
) -> Result<ExecutionResult, ExecError> {
    info!("command started");
    let started = Instant::now();
    let spawned = spawn_grouped(workspace, spec)?;
    let mut child = spawned.child;

    let stdout_reader = spawn_capture_reader("stdout", spawned.stdout, options.max_stdout_bytes);
    let stderr_reader = spawn_capture_reader("stderr", spawned.stderr, options.max_stderr_bytes);

    let (status, timed_out) = match wait_for_exit(&mut child, spec, started, options.timeout) {
        Ok(result) => result,
        Err(control_error) => {
            if best_effort_terminate(&mut child, &spec.program) {
                let _ = join_capture_reader("stdout", stdout_reader);
                let _ = join_capture_reader("stderr", stderr_reader);
            }
            return Err(control_error);
        }
    };

    let stdout_result = join_capture_reader("stdout", stdout_reader);
    let stderr_result = join_capture_reader("stderr", stderr_reader);
    let stdout = stdout_result?;
    let stderr = stderr_result?;
    let duration = started.elapsed();

    info!(
        exit_code = status.code(),
        duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        timed_out,
        stdout_truncated = stdout.truncated,
        stderr_truncated = stderr.truncated,
        "command finished"
    );

    Ok(ExecutionResult {
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
        duration,
        timed_out,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

fn wait_for_exit(
    child: &mut GroupChild,
    spec: &CommandSpec,
    started: Instant,
    timeout: Option<Duration>,
) -> Result<(ExitStatus, bool), ExecError> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|source| ExecError::ProcessFailed {
                message: format!("could not query command {}", spec.program),
                source,
            })?
        {
            return Ok((status, false));
        }

        if let Some(limit) = timeout {
            let elapsed = started.elapsed();
            if elapsed >= limit {
                if let Some(status) =
                    child
                        .try_wait()
                        .map_err(|source| ExecError::ProcessFailed {
                            message: format!("could not query command {}", spec.program),
                            source,
                        })?
                {
                    return Ok((status, false));
                }

                let status =
                    terminate_group(child, &format!("timed out command {}", spec.program))?;
                return Ok((status, true));
            }

            thread::sleep(WAIT_POLL_INTERVAL.min(limit.saturating_sub(elapsed)));
        } else {
            thread::sleep(WAIT_POLL_INTERVAL);
        }
    }
}

#[derive(Debug)]
struct CapturedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_capture_reader(
    stream_name: &'static str,
    stream: impl Read + Send + 'static,
    limit: usize,
) -> JoinHandle<CapturedStream> {
    thread::spawn(move || capture_stream(stream_name, stream, limit))
}

fn capture_stream(
    stream_name: &'static str,
    mut stream: impl Read,
    limit: usize,
) -> CapturedStream {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut chunk = [0_u8; 8192];

    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = limit.saturating_sub(bytes.len());
                let keep = remaining.min(count);
                bytes.extend_from_slice(&chunk[..keep]);
                truncated |= keep < count;
            }
            Err(error) => {
                warn!(stream = stream_name, error = %error, "command output read failed");
                break;
            }
        }
    }

    CapturedStream { bytes, truncated }
}

fn join_capture_reader(
    stream_name: &'static str,
    reader: JoinHandle<CapturedStream>,
) -> Result<CapturedStream, ExecError> {
    reader.join().map_err(|_| ExecError::ProcessFailed {
        message: format!("{stream_name} reader thread panicked"),
        source: std::io::Error::other("command output reader thread panicked"),
    })
}

fn best_effort_terminate(child: &mut GroupChild, program: &str) -> bool {
    match terminate_group(child, &format!("command {program} after control failure")) {
        Ok(_) => true,
        Err(error) => {
            warn!(
                program = %program,
                error = %error,
                "failed to clean up command after control failure"
            );
            false
        }
    }
}
