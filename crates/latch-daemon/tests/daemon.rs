use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};

use latch_local::LocalStore;
use latch_protocol::{
    ErrorCode, ProcessStateResponse, ResponseEnvelope, ResponseOutcome, ResponsePayload,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};

struct DaemonClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl DaemonClient {
    fn start() -> Self {
        Self::spawn(None)
    }

    fn start_with_state_dir(state_dir: &Path) -> Self {
        Self::spawn(Some(state_dir))
    }

    fn spawn(state_dir: Option<&Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_latch-daemon"));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(state_dir) = state_dir {
            command.env("LATCH_LOCAL_STATE_DIR", state_dir);
        }
        let mut child = command.spawn().unwrap();

        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());

        Self {
            child,
            stdin: Some(stdin),
            stdout,
        }
    }

    fn request(&mut self, id: &str, method: &str, params: &Value) -> ResponseEnvelope {
        self.send_value(&json!({
            "id": id,
            "version": PROTOCOL_VERSION,
            "method": method,
            "params": params
        }))
    }

    fn send_value(&mut self, request: &Value) -> ResponseEnvelope {
        self.send_raw(&serde_json::to_string(request).unwrap())
    }

    fn send_raw(&mut self, request: &str) -> ResponseEnvelope {
        let stdin = self.stdin.as_mut().expect("daemon stdin should be open");
        stdin.write_all(request.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).unwrap();
        assert_ne!(read, 0, "daemon closed stdout before responding");
        serde_json::from_str(&line).unwrap()
    }

    fn shutdown(&mut self) -> ExitStatus {
        drop(self.stdin.take());
        self.child.wait().unwrap()
    }
}

impl Drop for DaemonClient {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn success(response: ResponseEnvelope) -> ResponsePayload {
    match response.outcome {
        ResponseOutcome::Ok { result } => result,
        ResponseOutcome::Error { error } => panic!("expected success, got {error:?}"),
    }
}

fn error(response: ResponseEnvelope) -> latch_protocol::ProtocolError {
    match response.outcome {
        ResponseOutcome::Ok { result } => panic!("expected error, got {result:?}"),
        ResponseOutcome::Error { error } => error,
    }
}

fn approved_root(state_dir: &Path, workspace_path: &Path) -> latch_local::ApprovedRoot {
    std::fs::create_dir_all(workspace_path).unwrap();
    LocalStore::new(state_dir).add_root(workspace_path).unwrap()
}

#[cfg(windows)]
fn run_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "cmd",
        "args": ["/C", "echo daemon-run"]
    })
}

#[cfg(not(windows))]
fn run_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "sh",
        "args": ["-c", "echo daemon-run"]
    })
}

#[cfg(windows)]
fn start_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "cmd",
        "args": ["/C", "echo ready && ping -n 30 127.0.0.1 > nul"]
    })
}

#[cfg(not(windows))]
fn start_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "sh",
        "args": ["-c", "echo ready; sleep 30"]
    })
}

#[cfg(windows)]
fn delayed_marker_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "powershell",
        "args": [
            "-NoProfile",
            "-Command",
            "Start-Sleep -Milliseconds 700; Set-Content -Path daemon-survived.txt -Value survived"
        ]
    })
}

#[cfg(not(windows))]
fn delayed_marker_request(workspace_id: latch_core::WorkspaceId) -> Value {
    json!({
        "workspace_id": workspace_id,
        "program": "sh",
        "args": ["-c", "sleep 0.7; printf survived > daemon-survived.txt"]
    })
}

#[test]
fn daemon_exercises_v05_protocol_across_process_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let workspace_path = temp.path().join("workspace");
    let root = approved_root(&state_dir, &workspace_path);
    let mut daemon = DaemonClient::start_with_state_dir(&state_dir);

    let workspace_id = match success(daemon.request(
        "1",
        "workspace.open",
        &json!({"root_id": root.root_id, "relative_path": "."}),
    )) {
        ResponsePayload::Workspace(response) => response.workspace_id,
        other => panic!("unexpected workspace response: {other:?}"),
    };

    assert!(matches!(
        success(daemon.request(
            "2",
            "fs.write",
            &json!({
                "workspace_id": workspace_id,
                "path": "hello.txt",
                "contents": "hello from daemon"
            }),
        )),
        ResponsePayload::Ack
    ));

    match success(daemon.request(
        "3",
        "fs.read",
        &json!({"workspace_id": workspace_id, "path": "hello.txt"}),
    )) {
        ResponsePayload::FileContent(response) => {
            assert_eq!(response.contents, "hello from daemon");
        }
        other => panic!("unexpected read response: {other:?}"),
    }

    match success(daemon.request("4", "exec.run", &run_request(workspace_id))) {
        ResponsePayload::Exec(response) => {
            assert_eq!(response.exit_code, Some(0));
            assert!(response.stdout.contains("daemon-run"));
            assert!(!response.timed_out);
            assert!(!response.stdout_truncated);
            assert!(!response.stderr_truncated);
        }
        other => panic!("unexpected exec.run response: {other:?}"),
    }

    let job_id = match success(daemon.request("5", "exec.start", &start_request(workspace_id))) {
        ResponsePayload::ProcessStarted(response) => response.job_id,
        other => panic!("unexpected exec.start response: {other:?}"),
    };

    let mut saw_ready = match success(daemon.request("6", "exec.poll", &json!({"job_id": job_id})))
    {
        ResponsePayload::ProcessPoll(response) => {
            assert!(matches!(response.state, ProcessStateResponse::Running));
            response.stdout.text.contains("ready")
        }
        other => panic!("unexpected exec.poll response: {other:?}"),
    };

    for attempt in 0..20 {
        if saw_ready {
            break;
        }
        match success(daemon.request(
            &format!("poll-{attempt}"),
            "exec.poll",
            &json!({"job_id": job_id}),
        )) {
            ResponsePayload::ProcessPoll(response) => {
                if response.stdout.text.contains("ready") {
                    saw_ready = true;
                    break;
                }
            }
            other => panic!("unexpected exec.poll response: {other:?}"),
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(saw_ready, "managed output never became observable");

    match success(daemon.request("7", "exec.kill", &json!({"job_id": job_id}))) {
        ResponsePayload::ProcessPoll(response) => {
            assert!(matches!(
                response.state,
                ProcessStateResponse::Killed | ProcessStateResponse::Exited { .. }
            ));
        }
        other => panic!("unexpected exec.kill response: {other:?}"),
    }

    let traversal = error(daemon.request(
        "8",
        "fs.read",
        &json!({"workspace_id": workspace_id, "path": "../outside.txt"}),
    ));
    assert_eq!(traversal.code, ErrorCode::PathEscape);

    assert!(daemon.shutdown().success());
}

#[test]
fn daemon_reports_transport_protocol_errors() {
    let mut daemon = DaemonClient::start();

    let malformed = error(daemon.send_raw("{not-json"));
    assert_eq!(malformed.code, ErrorCode::InvalidRequest);

    let unsupported = error(daemon.send_value(&json!({
        "id": "unsupported",
        "version": PROTOCOL_VERSION + 1,
        "method": "roots.list",
        "params": {}
    })));
    assert_eq!(unsupported.code, ErrorCode::UnsupportedVersion);

    assert!(daemon.shutdown().success());
}

#[test]
fn daemon_shutdown_terminates_managed_processes() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let workspace_path = temp.path().join("workspace");
    let root = approved_root(&state_dir, &workspace_path);
    let marker = workspace_path.join("daemon-survived.txt");
    let mut daemon = DaemonClient::start_with_state_dir(&state_dir);

    let workspace_id = match success(daemon.request(
        "shutdown-open",
        "workspace.open",
        &json!({"root_id": root.root_id, "relative_path": "."}),
    )) {
        ResponsePayload::Workspace(response) => response.workspace_id,
        other => panic!("unexpected workspace response: {other:?}"),
    };

    let _job_id = match success(daemon.request(
        "shutdown-start",
        "exec.start",
        &delayed_marker_request(workspace_id),
    )) {
        ResponsePayload::ProcessStarted(response) => response.job_id,
        other => panic!("unexpected exec.start response: {other:?}"),
    };

    assert!(daemon.shutdown().success());
    thread::sleep(Duration::from_millis(1_100));
    assert!(
        !marker.exists(),
        "managed process survived daemon shutdown and wrote its marker"
    );
}
