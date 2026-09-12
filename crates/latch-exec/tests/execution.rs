use std::{path::PathBuf, thread, time::Duration};

use latch_core::{ProcessId, Workspace};
use latch_exec::{run_blocking, CommandSpec, ExecError, ProcessManager, ProcessState};

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(temp.path()).unwrap();
    (temp, workspace)
}

#[cfg(windows)]
fn shell(script: &str) -> CommandSpec {
    CommandSpec::new("cmd", ["/C", script])
}

#[cfg(not(windows))]
fn shell(script: &str) -> CommandSpec {
    CommandSpec::new("sh", ["-c", script])
}

#[test]
fn runs_successfully_and_captures_stdout() {
    let (_temp, workspace) = workspace();
    let result = run_blocking(&workspace, &shell("echo latch")).unwrap();

    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("latch"));
}

#[test]
fn captures_stderr_and_non_zero_exit() {
    let (_temp, workspace) = workspace();
    let result = run_blocking(&workspace, &shell("echo failure 1>&2 && exit 7")).unwrap();

    assert_eq!(result.exit_code, Some(7));
    assert!(result.stderr.contains("failure"));
}

#[test]
fn uses_workspace_as_working_directory() {
    let (_temp, workspace) = workspace();
    #[cfg(windows)]
    let command = shell("cd");
    #[cfg(not(windows))]
    let command = shell("pwd");

    let result = run_blocking(&workspace, &command).unwrap();
    let reported = PathBuf::from(result.stdout.trim()).canonicalize().unwrap();
    assert_eq!(reported, workspace.root());
}

#[test]
fn starts_queries_outputs_and_terminates_managed_process() {
    let (_temp, workspace) = workspace();
    let manager = ProcessManager::new();
    #[cfg(windows)]
    let command = shell("echo ready && ping -n 30 127.0.0.1 > nul");
    #[cfg(not(windows))]
    let command = shell("echo ready; sleep 30");

    let process_id = manager.start(&workspace, &command).unwrap();
    assert!(matches!(
        manager.status(process_id).unwrap().state,
        ProcessState::Running
    ));

    let mut saw_output = false;
    for _ in 0..20 {
        if manager
            .output(process_id)
            .unwrap()
            .stdout
            .text
            .contains("ready")
        {
            saw_output = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(saw_output, "managed stdout should become observable");

    let status = manager.kill(process_id).unwrap();
    assert!(matches!(status.state, ProcessState::Exited { .. }));

    let mut streams_complete = false;
    for _ in 0..40 {
        let output = manager.output(process_id).unwrap();
        if output.stdout.complete && output.stderr.complete {
            streams_complete = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        streams_complete,
        "terminating a managed process should close descendant-owned output pipes"
    );
}

#[test]
fn invalid_process_id_is_structured_error() {
    let manager = ProcessManager::new();
    let process_id = ProcessId::new();
    let error = manager.status(process_id).unwrap_err();
    assert!(matches!(error, ExecError::ProcessNotFound { .. }));
}
