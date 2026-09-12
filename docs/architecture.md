# Latch Local V0.1 Architecture

## Scope

V0.1 proves the local execution foundation only. It intentionally excludes HTTP, WebSockets, Vercel, ChatGPT integration, MCP, browser automation, computer use, authentication, and UI. Those layers must be able to arrive later without changing the local domain APIs.

## Crate boundaries

### `latch-core`

Owns stable domain identity and workspace concepts:

- `Workspace`
- `WorkspaceId`
- `ProcessId`
- workspace opening/creation errors

A workspace root is canonicalized when opened. Other crates receive the canonical root through `Workspace`; they do not independently invent workspace roots.

### `latch-fs`

Owns all Latch-native filesystem operations. The ambient workspace path is converted once into a `cap_std::fs::Dir`. After that boundary, file operations are descriptor/capability-relative rather than ambient path operations.

The public operations are intentionally small: text read/write, file deletion, recursive directory creation, directory listing, existence checks, and metadata.

### `latch-exec`

Owns process execution.

Blocking execution uses a process group, waits for completion, and returns exit code, bounded stdout/stderr, duration, timeout state, and truncation state. The default policy is a 5 minute timeout with 1 MiB retained independently for stdout and stderr. Reader threads drain both pipes concurrently even after a limit is reached, avoiding pipe backpressure deadlocks.

Managed execution stores a process group behind a generated `ProcessId` and supports start, status, output, kill, and manager-wide shutdown. Managed stdout/stderr are collected concurrently; each stream retains at most the latest 1 MiB.

### `latch-protocol`

Contains only serializable, versioned request/response DTOs and protocol error codes. It does not know about stdin/stdout, HTTP, WebSockets, Vercel, ChatGPT, or MCP.

V0.1 protocol methods:

```text
workspace.open
workspace.create
fs.read
fs.write
fs.delete
fs.mkdir
fs.list
exec.run
exec.start
exec.status
exec.output
exec.kill
```

### `latch-daemon`

Composes the other crates. It keeps an in-memory workspace registry and process manager, maps implementation errors into stable protocol errors, and exposes the engine as newline-delimited JSON over stdin/stdout.

Stdout is reserved for protocol responses. Structured `tracing` logs are emitted to stderr. When the stdin/stdout transport returns, including on normal EOF or a transport error, the daemon explicitly shuts down all managed process groups before returning from `main`.

## Dependency direction

```text
                  latch-core
             /        |        \
      latch-fs    latch-exec    latch-protocol
             \        |        /
                  latch-daemon
```

More precisely, `latch-daemon` depends on all four libraries; `latch-fs` and `latch-exec` depend on `latch-core`; `latch-protocol` depends only on `latch-core`. There are no circular dependencies.

## Workspace security model

An explicitly opened directory is the filesystem capability boundary.

1. `Workspace` canonicalizes the selected root once.
2. `latch-fs` opens that root as a `cap_std::fs::Dir` using ambient authority exactly at construction.
3. Every user path must be relative. Absolute paths, drive/UNC roots, and `..` components are rejected before filesystem access.
4. A canonicalization check provides deterministic rejection/logging for already-resolvable symlink or junction escapes.
5. The actual operation still goes through the capability-scoped directory handle, so security does not rely only on string-prefix checks.

This is why a path such as `../../secret.txt`, an absolute `C:\\Users\\...` path, or an in-workspace symlink/junction that resolves outside the workspace cannot be used through Latch's filesystem API.

### Important execution boundary

V0.1 does **not** OS-sandbox arbitrary child processes. Commands start with the workspace as their working directory, but a program the user explicitly launches retains that user's normal operating-system filesystem and network permissions. Restricting arbitrary subprocess access requires a separate process-sandbox design and is deliberately not faked by V0.1.

## Execution model

`exec.run` is synchronous and uses direct argv (`program` plus an argument array), not an implicit shell string. Shell behavior only occurs when the caller explicitly launches `cmd`, PowerShell, `sh`, etc.

Each blocking command runs as a process group: a Windows Job Object on Windows or a POSIX process group on Unix. Stdout and stderr are drained on separate threads into bounded capture buffers. The V0.1 protocol uses the default 5 minute / 1 MiB-per-stream policy. If the timeout expires, Latch terminates and waits for the full process group, then returns the bounded output collected up to termination with `timed_out: true`. Exit code remains populated when the operating system provides one.

`exec.start` uses the same process-group boundary with piped stdout/stderr and null stdin. Reader threads continuously drain both pipes into bounded buffers. The manager retains process records so status/output remain queryable after exit.

`exec.kill` terminates and reaps the managed process group rather than only the direct child. `ProcessManager::shutdown_all` snapshots the managed process handles, then performs best-effort cleanup process-by-process without holding the global registry mutex across kill/wait operations. Failures are logged and do not stop later processes from being cleaned up. Reader threads are joined after termination. `ProcessManager::Drop` calls the same idempotent cleanup path as a fallback, while the daemon invokes shutdown explicitly.

On Unix, a child that deliberately creates a new session/process group can escape this containment boundary. Forced owner termination such as `SIGKILL` also cannot run Rust destructors. V0.1 does not present process grouping as a full OS sandbox.

## Protocol model

Every request includes:

- caller-chosen request `id`
- protocol `version`
- strongly tagged `method`
- typed `params`

Responses echo the request ID and return either a typed result or a stable protocol error code. `exec.run` responses additionally report `timed_out`, `stdout_truncated`, and `stderr_truncated`. Internal Rust error sources remain available for local debugging, while the wire model avoids serializing arbitrary error chains.

The protocol is transport-neutral. A later local socket, HTTP adapter, plugin router, or another transport can translate bytes into the same `RequestEnvelope`, call the same engine, and serialize the same `ResponseEnvelope`.

## Why networking is excluded

Networking would introduce authentication, origin trust, discovery, lifecycle, encryption, and remote-attack-surface questions before the local engine has proven its semantics. V0.1 therefore keeps the transport local and process-bound. The protocol crate is the seam that allows networking to be added later without coupling it to filesystem or process logic.

## Known V0.1 limits

- workspaces and managed-process records are in-memory only
- managed output is a bounded tail, not a durable log store
- blocking output retains the first bounded portion of each stream, not a durable log
- arbitrary child commands are not OS-sandboxed beyond their working directory
- filesystem API is text-oriented for V0.1; binary transfer is intentionally not yet exposed
