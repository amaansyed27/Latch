use latch_core::{
    runtime_events::{self, ResourceKey},
    SessionId, Workspace,
};
use latch_exec::{CommandSpec, ProcessManager};

#[test]
fn managed_process_exit_wakes_event_wait_without_poll() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(temp.path()).unwrap();
    let manager = ProcessManager::new();
    let session_id = SessionId::new();
    let cursor = runtime_events::latest_sequence();

    #[cfg(windows)]
    let spec = CommandSpec::new("cmd.exe", ["/C", "exit", "0"]);
    #[cfg(not(windows))]
    let spec = CommandSpec::new("/bin/sh", ["-c", "exit 0"]);

    let started = manager.start_detailed(&workspace, &spec).unwrap();
    runtime_events::bind_resource(ResourceKey::Process(started.process_id), session_id);

    let events = runtime_events::read(
        session_id,
        cursor,
        &["process.exited".to_owned()],
        5_000,
        10,
    );
    assert!(events
        .iter()
        .any(|event| event.event_type == "process.exited"));

    manager.shutdown_all();
    runtime_events::clear_session(session_id);
}
