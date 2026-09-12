use std::process::{ChildStderr, ChildStdout, Command, ExitStatus, Stdio};

use command_group::{CommandGroup, GroupChild};
use latch_core::Workspace;
use tracing::warn;

use crate::{CommandSpec, ExecError};

pub(crate) struct SpawnedGroup {
    pub child: GroupChild,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
}

pub(crate) fn spawn_grouped(
    workspace: &Workspace,
    spec: &CommandSpec,
) -> Result<SpawnedGroup, ExecError> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(workspace.root())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = spawn_process_group(&mut command)
        .map_err(|source| ExecError::spawn(&spec.program, source))?;

    let stdout = child.inner().stdout.take();
    let stderr = child.inner().stderr.take();

    match (stdout, stderr) {
        (Some(stdout), Some(stderr)) => Ok(SpawnedGroup {
            child,
            stdout,
            stderr,
        }),
        _ => {
            cleanup_incomplete_spawn(&mut child, &spec.program);
            Err(ExecError::ProcessFailed {
                message: format!("output pipes were not available for {}", spec.program),
                source: std::io::Error::other("output pipe missing after spawn"),
            })
        }
    }
}

pub(crate) fn terminate_group(
    child: &mut GroupChild,
    description: &str,
) -> Result<ExitStatus, ExecError> {
    if let Err(source) = child.kill() {
        if source.kind() == std::io::ErrorKind::InvalidInput {
            let status = child
                .try_wait()
                .map_err(|query_source| ExecError::ProcessFailed {
                    message: format!("could not query {description} after termination race"),
                    source: query_source,
                })?;
            if let Some(status) = status {
                return Ok(status);
            }
        }

        return Err(ExecError::ProcessFailed {
            message: format!("could not terminate {description}"),
            source,
        });
    }

    child.wait().map_err(|source| ExecError::ProcessFailed {
        message: format!("could not wait for {description} after termination"),
        source,
    })
}

#[cfg(windows)]
fn spawn_process_group(command: &mut Command) -> std::io::Result<GroupChild> {
    command.group().kill_on_drop(true).spawn()
}

#[cfg(not(windows))]
fn spawn_process_group(command: &mut Command) -> std::io::Result<GroupChild> {
    command.group_spawn()
}

fn cleanup_incomplete_spawn(child: &mut GroupChild, program: &str) {
    if let Err(error) = child.kill() {
        warn!(program = %program, error = %error, "failed to terminate process after incomplete spawn");
    }
    if let Err(error) = child.wait() {
        warn!(program = %program, error = %error, "failed to reap process after incomplete spawn");
    }
}
