use std::{path::PathBuf, thread, time::Duration};

use latch_core::{ProcessId, Workspace};
use latch_exec::{
    run_blocking, run_blocking_with_options, CommandSpec, ExecError, ProcessManager, ProcessState,
    RunOptions, DEFAULT_RUN_OUTPUT_BYTES, DEFAULT_RUN_TIMEOUT,
};

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

#[cfg(windows)]
fn large_output_command() -> CommandSpec {
    CommandSpec::new(
        "powershell",
        [
            "-NoProfile",
            "-Command",
            "$out = 'o' * 131072; $err = 'e' * 131072; [Console]::Out.Write($out); [Console]::Error.Write($err)",
        ],
    )
}

#[cfg(not(windows))]
fn large_output_command() -> CommandSpec {
    CommandSpec::new(
        "python3",
        [
            "-c",
            "import sys; sys.stdout.write('o'*131072); sys.stderr.write('e'*131072)",
        ],
    )
}

#[cfg(windows)]
fn delayed_marker_command() -> CommandSpec {
    CommandSpec::new(
        "powershell",
        [
            "-NoProfile",
            "-Command",
            "Start-Sleep -Milliseconds 700; Set-Content -Path drop-survived.txt -Value survived",
        ],
    )
}

#[cfg(not(windows))]
fn delayed_marker_command() -> CommandSpec {
    shell("sleep 0.7; printf survived > drop-survived.txt")
}

#[test]
fn run_options_have_bounded_defaults() {
    let options = RunOptions::default();
    assert_eq!(options.timeout, Some(DEFAULT_RUN_TIMEOUT));
    assert_eq!(options.max_stdout_bytes, DEFAULT_RUN_OUTPUT_BYTES);
    assert_eq!(options.max_stderr_bytes, DEFAULT_RUN_OUTPUT_BYTES);
    assert_eq!(DEFAULT_RUN_TIMEOUT, Duration::from_secs(5 * 60));
    assert_eq!(DEFAULT_RUN_OUTPUT_BYTES, 1024 * 1024);
}

#[test]
fn runs_successfully_and_captures_stdout() {
    let (_temp, workspace) = workspace();
    let result = run_blocking(&workspace, &shell("echo latch")).unwrap();

    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("latch"));
    assert!(!result.timed_out);
    assert!(!result.stdout_truncated);
    assert!(!result.stderr_truncated);
}

#[test]
fn captures_stderr_and_non_zero_exit() {
    let (_temp, workspace) = workspace();
    let result = run_blocking(&workspace, &shell("echo failure 1>&2 && exit 7")).unwrap();

    assert_eq!(result.exit_code, Some(7));
    assert!(result.stderr.contains("failure"));
    assert!(!result.timed_out);
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
fn blocking_run_times_out_and_returns_partial_output() {
    let (_temp, workspace) = workspace();
    #[cfg(windows)]
    let command = shell("echo before-timeout && ping -n 30 127.0.0.1 > nul");
    #[cfg(not(windows))]
    let command = shell("echo before-timeout; sleep 30");

    let options = RunOptions {
        timeout: Some(Duration::from_secs(1)),
        max_stdout_bytes: 4096,
        max_stderr_bytes: 4096,
    };
    let result = run_blocking_with_options(&workspace, &command, &options).unwrap();

    assert!(result.timed_out);
    assert!(result.stdout.contains("before-timeout"));
    assert!(result.duration < Duration::from_secs(5));
}

#[test]
fn blocking_run_bounds_and_drains_both_output_streams() {
    let (_temp, workspace) = workspace();
    let options = RunOptions {
        timeout: Some(Duration::from_secs(15)),
        max_stdout_bytes: 1024,
        max_stderr_bytes: 1024,
    };

    let result = run_blocking_with_options(&workspace, &large_output_command(), &options).unwrap();

    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout.len(), 1024);
    assert_eq!(result.stderr.len(), 1024);
    assert!(result.stdout_truncated);
    assert!(result.stderr_truncated);
    assert!(!result.timed_out);
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
    assert!(matches!(status.state, ProcessState::Killed));

    let output = manager.output(process_id).unwrap();
    assert!(output.stdout.complete);
    assert!(output.stderr.complete);
}

#[test]
fn shutdown_all_terminates_running_process_tree() {
    let (_temp, workspace) = workspace();
    let manager = ProcessManager::new();
    #[cfg(windows)]
    let command = shell("echo ready && ping -n 30 127.0.0.1 > nul");
    #[cfg(not(windows))]
    let command = shell("echo ready; sleep 30");

    let process_id = manager.start(&workspace, &command).unwrap();
    let report = manager.shutdown_all();

    assert_eq!(report.failures, 0);
    assert_eq!(report.terminated, 1);
    assert!(matches!(
        manager.status(process_id).unwrap().state,
        ProcessState::Killed
    ));

    let output = manager.output(process_id).unwrap();
    assert!(
        output.stdout.complete && output.stderr.complete,
        "shutdown must close descendant-owned output pipes"
    );
}

#[test]
fn drop_fallback_terminates_running_process() {
    let (temp, workspace) = workspace();
    let marker = temp.path().join("drop-survived.txt");

    {
        let manager = ProcessManager::new();
        manager
            .start(&workspace, &delayed_marker_command())
            .unwrap();
    }

    thread::sleep(Duration::from_secs(1));
    assert!(
        !marker.exists(),
        "managed process survived ProcessManager drop and wrote its marker"
    );
}

#[test]
fn invalid_process_id_is_structured_error() {
    let manager = ProcessManager::new();
    let process_id = ProcessId::new();
    let error = manager.status(process_id).unwrap_err();
    assert!(matches!(error, ExecError::ProcessNotFound { .. }));
}
