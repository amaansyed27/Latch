# Latch V0.5 Router

## Purpose

The V0.5 Router is Latch's authenticated remote control plane. It exposes the MCP endpoint used by ChatGPT, Codex, and other compatible clients, verifies authorization and device ownership, and routes typed protocol-v2 requests to the correct paired computer.

```text
ChatGPT / Codex / MCP client
        |
        | HTTPS Streamable HTTP + OAuth
        v
      /mcp
        |
        v
      Router
        |
        | Redis presence + request/response routing
        v
outbound TLS WebSocket
        |
        v
    latch-link
        |
        v
   latch-engine
```

The Router is deliberately thin. It is not a shell host, build host, filesystem store, or local MCP configuration store. File access, process execution, computer use, and local MCP calls execute on the user's paired computer.

## Outbound-only device connectivity

Each paired computer runs `latch-link`, which initiates an outbound TLS WebSocket connection to the Router. Latch does not require port forwarding, a public localhost tunnel, or an inbound listener on the user's computer.

Connected-device presence is ephemeral. Redis is used to route requests to the Vercel Function instance currently holding a device socket and to correlate the corresponding response. A disconnected device reconnects with bounded backoff.

## Accounts, pairing, and device credentials

Users sign in through the Latch web experience. From **Devices → Add computer**, the user creates a short-lived one-time pairing code and pastes it into Latch Desktop.

The pairing exchange binds a local `DeviceId`/device name to the signed-in Latch account and returns a device credential. On Windows, the credential is stored in Windows Credential Manager; the device identity is persisted under the local Latch application-data directory.

Pairing codes are single-use and time-limited. The desktop UI is the primary pairing path. The CLI remains a fallback/developer interface.

Revoking a computer from the Devices page invalidates its credential and prevents new routed requests for that device.

## MCP and OAuth

Production MCP endpoint:

```text
https://latch-router.vercel.app/mcp
```

The endpoint uses OAuth and advertises capability-specific scopes. Current scope groups cover:

- connected devices
- approved roots/workspaces
- file reads
- file edits
- command execution/process management
- screen discovery/capture
- mouse/keyboard control
- local MCP discovery
- local MCP execution

OAuth permission is only the remote authorization layer. The local permission switches in Latch Desktop are an additional deny layer and take precedence over previously granted OAuth scopes.

Before relaying a device-targeted request, the Router verifies both the required OAuth scope and that the selected device belongs to the authenticated account.

## MCP tool boundary

The Router exposes named, typed MCP tools. It does not expose a generic raw Latch-protocol passthrough tool.

Normal workspace opening accepts an approved `root_id` plus an optional relative subpath. The ChatGPT-facing schema does not accept an arbitrary absolute OS path and does not expose the local-only `workspace.open_raw` compatibility request.

Tool results are mapped back to MCP content. Explicit screenshot results are returned as MCP image content; file, command, and local MCP output must be treated as untrusted data.

## Protocol version 2

The Router relays Latch protocol **version 2** envelopes to `latch-link`.

Each relay operation uses independent correlation identifiers so concurrent requests can be matched to their responses. Stable local error codes are preserved where useful, including permission, workspace-expiry, filesystem, process, computer-use, and local-MCP errors.

The Router does not duplicate local capability policy. `latch-engine` remains responsible for current approved-root state, local pause, and local permission enforcement.

## Redis routing

Redis is used only as a coordination layer for the distributed Router runtime:

- expiring device presence
- instance dispatch routing
- request/response correlation
- rate-limit state

It is not intended as durable storage for source files, screenshots, command output, or local MCP configuration.

The in-memory coordinator is used by automated tests and the local E2E harness.

## Router persistence boundary

Durable account/authorization storage contains the metadata required for the Latch account, paired devices, OAuth clients/grants, and protected token state.

The Router does **not intentionally durably store**:

- project/source files
- workspace contents
- command stdout/stderr
- screenshots
- local MCP server configuration
- local MCP environment-variable references or resolved secret values

Content intentionally returned by a tool is delivered to the connected MCP client and is then subject to that client's own data policies.

## Local authority

Every routed operation is still checked locally by `latch-engine`.

Default local permissions in V0.5 are:

| Capability | Default |
| --- | --- |
| Files | ON |
| Commands | ON |
| Screen | OFF |
| Computer control | OFF |
| MCP discovery | OFF |
| MCP execution | OFF |

`Pause remote access` rejects new remote operations without deleting pairing.

Commands are **not sandboxed**. They run with the permissions of the local OS user running Latch and can access anything that account could access directly, even outside a filesystem workspace.

Screen capture occurs only when the screenshot tool is explicitly invoked; there is no periodic background screenshot capture.

## Production configuration

The Router requires its configured production authentication, authorization-store, and Redis settings. Secrets are supplied through the deployment environment and are not committed to the repository.

`LATCH_ALLOW_LEGACY_APP_TOKEN` is disabled unless explicitly set to `true`; production V0.5 uses OAuth for the normal MCP connection.

The public site, OAuth endpoints, MCP endpoint, and device relay run from the same `router/` Vercel project.

## Automated acceptance

CI validates the real routing path rather than bypassing the engine:

```text
Router -> authenticated device relay -> latch-link -> latch-engine
```

The E2E suite covers approved-root discovery/opening, file read/write, command execution, stable computer-use capability behavior on the CI platform, local MCP server discovery, local MCP tool discovery, and a local MCP tool call through a standards-based stdio fixture.

The Rust local MCP client also contains both stdio and Streamable HTTP transport implementations. Blender MCP and browser/Playwright MCP are supported through this generic bridge but are not yet claimed as manually verified third-party integrations for this beta.

## Security summary

- device ownership is checked before relay dispatch
- OAuth scopes are checked per MCP tool
- normal remote workspace opening uses approved roots, not raw absolute paths
- local permissions can deny access even after OAuth authorization
- screen capture is explicit-request only
- Router storage is not used for durable tool payloads
- local MCP configuration stays local
- command execution is powerful and intentionally unsandboxed

See [architecture.md](architecture.md) for the full crate and local-capability boundaries.
