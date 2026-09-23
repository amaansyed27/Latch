use latch_engine::Engine;
use latch_link::{execute_remote, ClientMessage};
use latch_local::LocalStore;
use latch_protocol::{
    ExecRequest, Request, RequestEnvelope, ResponseOutcome, ResponsePayload, WorkspaceOpenRequest,
    PROTOCOL_VERSION,
};

#[test]
fn remote_request_executes_through_shared_engine_and_preserves_correlation() {
    let temp = tempfile::tempdir().unwrap();
    let store = LocalStore::new(temp.path().join("state"));
    let root = store.add_root(temp.path()).unwrap();
    let engine = Engine::with_store(store);

    let open = engine.handle_envelope(RequestEnvelope {
        id: "open".to_owned(),
        version: PROTOCOL_VERSION,
        request: Request::WorkspaceOpen(WorkspaceOpenRequest {
            root_id: root.root_id,
            relative_path: Some(".".to_owned()),
        }),
    });
    let workspace_id = match open.outcome {
        ResponseOutcome::Ok {
            result: ResponsePayload::Workspace(workspace),
        } => workspace.workspace_id,
        other => panic!("unexpected workspace response: {other:?}"),
    };

    #[cfg(windows)]
    let (program, args) = (
        "cmd".to_owned(),
        vec!["/C".to_owned(), "echo latch-remote".to_owned()],
    );
    #[cfg(not(windows))]
    let (program, args) = (
        "sh".to_owned(),
        vec!["-c".to_owned(), "echo latch-remote".to_owned()],
    );

    let message = execute_remote(
        &engine,
        "remote-correlation-1".to_owned(),
        RequestEnvelope {
            id: "exec".to_owned(),
            version: PROTOCOL_VERSION,
            request: Request::ExecRun(ExecRequest {
                workspace_id,
                program,
                args,
            }),
        },
    );

    match message {
        ClientMessage::Response {
            request_id,
            response,
        } => {
            assert_eq!(request_id, "remote-correlation-1");
            match response.outcome {
                ResponseOutcome::Ok {
                    result: ResponsePayload::Exec(exec),
                } => {
                    assert_eq!(exec.exit_code, Some(0));
                    assert!(exec.stdout.contains("latch-remote"));
                }
                other => panic!("unexpected exec response: {other:?}"),
            }
        }
        ClientMessage::Hello { .. } => panic!("expected response message"),
    }
}
