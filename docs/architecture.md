# Latch Architecture

## V0.2 scope

V0.2 adds a secure remote relay around the V0.1 local engine. The Router is a control plane; it never executes commands or performs local filesystem operations.

```text
future ChatGPT Plugin
        |
        v
   Vercel Router
        |
        | outbound WebSocket from device
        v
     latch-link
        |
        v
    latch-engine
     /   |    \
latch-fs latch-exec latch-protocol
        |
   latch-daemon
    (stdio adapter)
```

`latch-daemon` and `latch-link` are transport adapters over the same `latch-engine`. The original stdin/stdout protocol remains supported.

## Crate boundaries

### `latch-core`

Owns `Workspace`, `WorkspaceId`, `ProcessId`, and `DeviceId` domain identity. Workspace roots are canonicalized once when opened.

### `latch-fs`

Owns Latch-native filesystem operations through a `cap_std::fs::Dir`. User paths must remain relative; absolute paths, parent traversal, and known symlink/junction escapes are rejected.

### `latch-exec`

Owns local process execution. `exec.run` uses process groups, concurrently drained bounded stdout/stderr, a five-minute default timeout, and one MiB retained independently per stream. Managed processes support start/status/output/kill and manager-wide cleanup.

### `latch-protocol`

Contains the versioned transport-neutral execution DTOs. V0.2 keeps protocol version 1 and does not duplicate these method types in the Router.

### `latch-engine`

Composes workspace state, `latch-fs`, `latch-exec`, and `latch-protocol`. It owns request execution and stable implementation-to-protocol error mapping. This is the small extraction from the V0.1 daemon that lets multiple transports reuse exactly the same local behavior.

### `latch-daemon`

Preserves the V0.1 newline-delimited stdin/stdout transport. It parses lines, delegates typed envelopes to `latch-engine`, writes protocol responses, and explicitly shuts the Engine down on transport exit.

### `latch-link`

Owns only remote-connection concerns: persisted `DeviceId`, configuration, authentication handshake, WebSocket lifecycle, request routing envelope, and bounded reconnect backoff. It does not implement filesystem or execution behavior.

The local connection is outbound-only. `https` Router URLs are upgraded to `wss` and no inbound local port is opened.

### `router/`

The TypeScript Vercel control plane exposes health/device/request HTTP APIs and accepts authenticated device WebSockets. It uses Redis for expiring device presence plus pub/sub dispatch/response correlation because a later HTTP Function invocation is not guaranteed to run on the Function instance holding a device socket.

## Dependency direction

```text
latch-core <- latch-fs
    ^        latch-exec
    +------- latch-protocol
       \       |       /
          latch-engine
          /          \
 latch-daemon      latch-link
```

The Router has no Rust crate dependency and treats the nested Latch protocol envelope as transport-neutral JSON.

## Workspace security model

An explicitly opened directory is the Latch filesystem capability boundary:

1. `Workspace` canonicalizes the selected root.
2. `latch-fs` opens the root as a capability directory.
3. Latch filesystem paths must be relative and may not contain `..`, drive/UNC roots, or absolute roots.
4. Known symlink/junction escapes are rejected and actual operations remain capability-relative.

Adding remote transport does not weaken this boundary.

## Important execution boundary

Latch does **not** OS-sandbox arbitrary child processes. Commands start in the selected workspace but retain the operating-system permissions of the local user. This remains true whether a request arrives over stdio or through the authenticated Router.

Therefore filesystem permission and command permission are not equivalent. V0.2 pairing/control secrets authorize access to the existing protocol; they are not a final fine-grained permission system.

## Execution lifecycle

Blocking and managed execution use a Windows Job Object or POSIX process group. Timeout/kill targets the group, and managed processes are terminated/reaped on Engine shutdown. On Unix, a process deliberately creating a new session/group may escape this lifecycle boundary; process groups are not presented as a full sandbox.

`latch-link` executes Engine requests on Tokio's blocking pool behind a small serialized Engine lock. This keeps WebSocket ping/pong and reconnect traffic responsive while synchronous local work is running.

## Remote routing model

A Router request contains the existing `RequestEnvelope`. The relay adds its own UUID `request_id` only for routing/correlation. Device responses return the same `request_id` plus the existing typed `ResponseEnvelope`.

Redis presence identifies the Vercel Function instance and connection that currently holds a device WebSocket. Dispatch is published to that instance. Responses are published on a request-specific channel and resolve only the matching pending HTTP request. Pending state is cleared on success, timeout, coordinator shutdown, or device disconnect.

## Authentication model

V0.2 uses two environment-provided random secrets:

- a pairing token for device WebSocket authentication
- a control token for Router device/request APIs

Secrets use timing-safe digest comparison and are excluded from normal logs/errors. TLS is provided by the Vercel `https`/`wss` endpoint.

There are no accounts, OAuth, billing, or per-method consent controls in V0.2.

## Known V0.2 limits

- workspaces and process records are still local in-memory state
- device presence is ephemeral, not durable history
- the proof-scale Redis registry is not designed as a large device directory
- command execution inherits local user OS permissions
- V0.2 has no final user permission/consent UI
- the Router request timeout can expire before the local execution timeout
- ChatGPT integration is intentionally deferred
