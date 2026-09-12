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

Blocking execution waits for completion and returns exit code, stdout, stderr, and duration. Managed execution stores a child process behind a generated `ProcessId` and supports start, status, output, and kill.

Managed stdout/stderr are collected concurrently. Each stream retains at most the latest 1 MiB and exposes a truncation flag so a noisy development server cannot grow daemon memory without bound.

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

Stdout is reserved for protocol responses. Structured `tracing` logs are emitted to stderr.

## Dependency direction

```text
latch-core
  ↑   ↑
  |   |
latch-fs   latch-exec
   \       /
    \     /
  latch-protocol   (depends only on latch-core)
        \          /
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

V0.1 does **not** OS-sandbox arbitrary child processes. Commands start with the workspace as their working directory, but a program the user explicitly launches retains that user's normal operating-system filesystem permissions. Restricting arbitrary subprocess filesystem/network access requires a separate process-sandbox design and is deliberately not faked by V0.1.

## Execution model

`exec.run` is synchronous. It uses direct argv (`program` plus an argument array), not an implicit shell string. Shell behavior only occurs when the caller explicitly launches `cmd`, PowerShell, `sh`, etc.

`exec.start` creates a managed process group with piped stdout/stderr and null stdin. On Windows, the group is backed by a Job Object; on Unix, it is a POSIX process group. Reader threads continuously drain both pipes into bounded buffers. The manager retains process records so status/output remain queryable after exit.

`exec.kill` terminates the managed process group rather than only the direct child. This is important for development commands such as `npm run dev` that normally spawn descendants. A Unix child that deliberately detaches itself into a new session/process group can still escape this boundary; V0.1 does not pretend to provide a full OS sandbox.

## Protocol model

Every request includes:

- caller-chosen request `id`
- protocol `version`
- strongly tagged `method`
- typed `params`

Responses echo the request ID and return either a typed result or a stable protocol error code. Internal Rust error sources remain available for local debugging, while the wire model avoids serializing arbitrary error chains.

The protocol is transport-neutral. A later local socket, HTTP adapter, plugin router, or another transport can translate bytes into the same `RequestEnvelope`, call the same engine, and serialize the same `ResponseEnvelope`.

## Why networking is excluded

Networking would introduce authentication, origin trust, discovery, lifecycle, encryption, and remote-attack-surface questions before the local engine has proven its semantics. V0.1 therefore keeps the transport local and process-bound. The protocol crate is the seam that allows networking to be added later without coupling it to filesystem or process logic.

## Known V0.1 limits

- workspaces and managed-process records are in-memory only
- managed output is a bounded tail, not a durable log store
- arbitrary child commands are not OS-sandboxed beyond their working directory
- filesystem API is text-oriented for V0.1; binary transfer is intentionally not yet exposed
