use std::{process::Command, time::Instant};

use latch_core::Workspace;
use tracing::{info, instrument};

use crate::{CommandSpec, ExecError, ExecutionResult};

#[instrument(skip(workspace, spec), fields(workspace_id = %workspace.id(), program = %spec.program))]
pub fn run_blocking(
    workspace: &Workspace,
    spec: &CommandSpec,
) -> Result<ExecutionResult, ExecError> {
    info!("command started");
    let started = Instant::now();
    let output = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(workspace.root())
        .output()
        .map_err(|source| ExecError::spawn(&spec.program, source))?;
    let duration = started.elapsed();

    info!(
        exit_code = output.status.code(),
        duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        "command finished"
    );

    Ok(ExecutionResult {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration,
    })
}
