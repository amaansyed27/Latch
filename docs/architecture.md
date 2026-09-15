# Latch Architecture

## V0.5 overview

Latch V0.5 lets an authorized ChatGPT session, Codex session, or other compatible MCP client use capabilities on a computer the user has explicitly paired and configured.

```text
ChatGPT / Codex / MCP client
        |
        | HTTPS Streamable HTTP + OAuth
        v
https://latch-router.vercel.app/mcp
        |
        v
      Router
        |
        | Redis presence + request/response routing
        v
outbound TLS WebSocket from the user's computer
        |
        v
    latch-link
        |
        v
   latch-engine
   /    |      |        \
 files process computer local MCP
```

The Router is a thin authenticated routing/control plane. It is not a build host, shell host, filesystem store, or durable store for tool payloads. Local execution remains on the paired computer.

## Authentication and routing

The public MCP endpoint uses OAuth. OAuth scopes are separated by capability, including devices, approved roots, files, commands, screen access, computer control, and local MCP access.

OAuth authorization is necessary but not sufficient. The local permission state on the paired computer is an additional deny layer and takes precedence over OAuth grants.

The Router authenticates the caller, verifies ownership of the selected device, and relays a typed request to the device that currently owns the outbound connection. Redis provides expiring device presence and request/response coordination so routing does not depend on one Vercel Function instance retaining a particular request.

The device connection is outbound-only. `latch-link` opens a TLS WebSocket to the Router; Latch does not require an inbound local port or expose localhost to the public internet.

## Protocol

V0.5 uses Latch protocol version **2**.

The protocol contains typed request/response DTOs and stable error codes for approved roots, filesystem operations, managed processes, computer use, and local MCP access. The Router does not expose a generic raw-protocol passthrough tool.

Normal remote workspace opening accepts:

- `root_id`
- optional `relative_path`

It does **not** accept an arbitrary absolute filesystem path.

A legacy raw absolute-workspace request exists only as a local/developer compatibility path. It is controlled by the local `legacy_absolute_workspaces` setting, defaults to `false`, and is not exposed by the normal ChatGPT-facing MCP tool schema.

## Local authority and approved roots

Approved roots are configured locally in Latch Desktop. Each approved root has an opaque `root_id`, display name, and canonical local path. Remote clients receive only the opaque ID and display name.

When an approved workspace is opened:

1. `latch-engine` resolves the requested `root_id` from current local configuration.
2. An optional relative subpath is validated.
3. The selected path is canonicalized and must remain inside the approved root.
4. `latch-fs` performs capability-relative operations through `cap_std`.
5. Absolute paths, parent traversal, drive/UNC escapes, and known symlink/junction escapes are rejected.
6. Later workspace operations re-check that the root is still approved. Removing the root invalidates the workspace.

Unexpected canonicalization failures fail closed rather than being treated as safe.

## Local permission model

Local permissions are persisted on the paired computer and checked on every relevant operation. V0.5 defaults are:

| Capability | Default |
| --- | --- |
| Files | ON |
| Commands | ON |
| Screen | OFF |
| Computer control | OFF |
| MCP discovery | OFF |
| MCP execution | OFF |

`Pause remote access` is another local deny switch. Pausing does not delete pairing; it causes new remote operations to be rejected locally.

Disabling a permission or removing an approved root takes effect independently of previously granted OAuth scopes. Already-running child processes are not automatically terminated solely because a later permission/root change occurred.

## Filesystem

`latch-fs` owns workspace-relative file access, including listing, stat, bounded reads, create/overwrite writes, deterministic patching, search, directory creation, move, and delete.

Filesystem confinement applies to Latch filesystem operations. It is not an operating-system sandbox for arbitrary programs.

## Processes

`latch-exec` owns blocking and managed process execution, including start, poll, stdin, output retention, and termination.

Commands are **not sandboxed**. They execute with the operating-system permissions of the local user running Latch. A command started inside an approved workspace can still access other resources that the same Windows user could access directly.

Process lifecycle control uses Windows Job Objects or POSIX process groups where supported. Output is bounded to avoid unbounded in-memory growth.

## Computer use

`latch-computer` owns display discovery, window discovery, explicit screenshots, focus, mouse movement/click/drag, scrolling, key input, and typing.

Screen and control permissions are separate. Screen capture is performed only when the screenshot tool is explicitly invoked. Latch does not periodically capture the desktop. Screenshot bytes are encoded in memory for the response and are not intentionally persisted by the Router.

Windows is the current primary computer-use implementation. Unsupported platforms return a stable `computer_unavailable`/unsupported result rather than pretending the action succeeded.

## Local MCP bridge

`latch-mcp-client` is a generic client for user-configured local MCP servers. V0.5 supports:

- stdio MCP transports
- Streamable HTTP MCP transports
- server discovery/status exposure through Latch
- tool discovery
- tool invocation

Local MCP configuration stays on the paired computer. The Router receives only the results needed for an authorized request; it does not become the configuration store for the user's local MCP servers.

For stdio servers, environment configuration stores **environment-variable references**, not raw secret values. Secret values are resolved from the local environment only when the server is launched.

Each local MCP server also has local `enabled` and `allow_remote` controls. Generic bridge behavior is verified by automated MCP fixtures and relay E2E tests. Specific third-party integrations such as Blender MCP and browser/Playwright MCP use this same generic bridge but require their own manual compatibility validation.

## Crate and package boundaries

### `latch-core`

Shared domain identities and core workspace/device types, including opaque workspace/root/process/device identifiers.

### `latch-local`

Persistent local authority: approved roots, capability permissions, pause state, local MCP configuration, and bounded activity metadata.

### `latch-fs`

Capability-confined filesystem primitives for an opened workspace.

### `latch-exec`

Blocking and managed local process execution, bounded output, stdin, polling, and process-tree termination.

### `latch-computer`

Local display/window discovery, explicit screenshots, and Windows mouse/keyboard control.

### `latch-mcp-client`

Generic local MCP client for stdio and Streamable HTTP transports.

### `latch-protocol`

Transport-neutral protocol version 2 request/response DTOs and stable error codes.

### `latch-engine`

The local execution authority. It composes local configuration, approved-root validation, filesystem/process/computer/MCP capabilities, permission checks, activity recording, and protocol error mapping.

### `latch-link`

The outbound remote transport. It owns device identity/credential use, WebSocket lifecycle, reconnect behavior, and delivery of Router requests to `latch-engine`. It does not reimplement capability logic.

### `latch-desktop`

Windows tray/desktop UI for pairing, approved folders, local MCP configuration, permission switches, pause/resume, activity, and diagnostics.

### `latch-daemon`

Compatibility stdin/stdout transport over the same `latch-engine`. It is not the primary V0.5 user experience.

### `router/`

TypeScript MCP/OAuth/website control plane deployed to Vercel. It owns OAuth, device ownership checks, MCP tool schemas, rate limiting, and relay coordination. Redis is used for transient routing/presence. The Router is intentionally not a local build executor or durable project/file store.

## End-to-end request path

A normal remote operation follows this sequence:

1. The MCP client connects to `/mcp` and authorizes requested OAuth scopes.
2. The Router authenticates the OAuth principal and checks the scope required by the selected tool.
3. Device ownership is verified before relay dispatch.
4. The Router publishes/routes the request to the currently connected device.
5. `latch-link` receives the request over its outbound TLS WebSocket.
6. `latch-engine` reloads current local authority and enforces pause, approved-root state, and local permissions.
7. The relevant local capability executes.
8. The typed result returns over the same relay path to the MCP client.

Returned file contents, command output, screenshots, and local MCP output are untrusted data. They are tool results, not policy or instructions for Latch itself.

## Persistence boundaries

Persisted locally:

- approved roots
- local permission state
- local MCP configuration
- device identity and credential material
- bounded activity/diagnostic state

Persisted by the control plane as required for accounts/authorization:

- account identity
- paired-device metadata
- OAuth grants/tokens in protected/hashed form as applicable

Transient routing data:

- device presence
- request/response coordination
- rate-limit state

Project files, screenshots, command output, and local MCP configuration are not intentionally stored as durable Router data.
