# Latch for ChatGPT

**Latch for ChatGPT — Your local machine, inside ChatGPT.**

Latch is intended to become a secure bridge between ChatGPT and a user's local machine. **V0.1 is deliberately smaller:** it is only the local workspace filesystem and process-execution foundation. There is no ChatGPT integration, networking, browser automation, MCP, computer use, or authentication yet.

## V0.1

- open or create explicit workspaces
- read, write, delete, create, and list workspace files/directories
- capability-scoped filesystem access with traversal and symlink-escape protection
- run blocking commands with bounded stdout/stderr, timeout, exit code, and duration
- start managed long-running process groups
- query managed process status/output and terminate processes
- clean up still-running managed processes when the daemon shuts down
- line-delimited JSON daemon over stdin/stdout

`exec.run` defaults to a **5 minute timeout** and retains at most **1 MiB each** of stdout and stderr while draining both streams concurrently. Its response reports `timed_out`, `stdout_truncated`, and `stderr_truncated`.

## Architecture

```text
                  latch-core
             /        |        \
      latch-fs    latch-exec    latch-protocol
             \        |        /
                  latch-daemon
                       |
                 stdin / stdout
```

`latch-protocol` depends only on `latch-core`; `latch-daemon` composes the filesystem, execution, and protocol crates. Filesystem and execution implementations never depend on a transport.

## Workspace

```text
Latch/
├── Cargo.toml
├── crates/
│   ├── latch-core/
│   ├── latch-fs/
│   ├── latch-exec/
│   ├── latch-protocol/
│   └── latch-daemon/
├── docs/
│   └── architecture.md
└── .github/workflows/ci.yml
```

## Build and test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace
```

Run the local daemon:

```bash
cargo run -p latch-daemon
```

Logs are JSON on stderr. Protocol responses are newline-delimited JSON on stdout. Normal daemon shutdown explicitly terminates and reaps still-running managed process groups; `ProcessManager` also has a `Drop` fallback.

## Example daemon interaction

Each input is one JSON line. On Windows, replace the example workspace with a path you want Latch to own.

```json
{"id":"1","version":1,"method":"workspace.create","params":{"path":"D:\\Temp\\latch-demo"}}
```

Use the returned `workspace_id` in later calls:

```json
{"id":"2","version":1,"method":"fs.write","params":{"workspace_id":"<workspace-id>","path":"hello.txt","contents":"hello from Latch"}}
{"id":"3","version":1,"method":"fs.read","params":{"workspace_id":"<workspace-id>","path":"hello.txt"}}
{"id":"4","version":1,"method":"exec.run","params":{"workspace_id":"<workspace-id>","program":"cmd","args":["/C","dir"]}}
{"id":"5","version":1,"method":"exec.start","params":{"workspace_id":"<workspace-id>","program":"powershell","args":["-NoProfile","-Command","while ($true) { Write-Output tick; Start-Sleep 2 }"]}}
{"id":"6","version":1,"method":"exec.status","params":{"process_id":"<process-id>"}}
{"id":"7","version":1,"method":"exec.output","params":{"process_id":"<process-id>"}}
{"id":"8","version":1,"method":"exec.kill","params":{"process_id":"<process-id>"}}
{"id":"9","version":1,"method":"fs.read","params":{"workspace_id":"<workspace-id>","path":"..\\outside.txt"}}
```

Request 9 must fail with `path_outside_workspace`.

Command execution is **not** a filesystem sandbox. Commands start in the workspace but retain the operating-system permissions of the user running Latch.

See [`docs/architecture.md`](docs/architecture.md) for design and security details.
