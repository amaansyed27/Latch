# Latch V0.2 Router

## Purpose

V0.2 proves one thing: an authenticated Router request can reach a connected user machine, execute through the existing Latch engine, and return the typed Latch protocol response.

```text
HTTP control request
      |
      v
Vercel Router ---- Redis presence/pubsub
      |
      | WebSocket over TLS
      v
  latch-link
      |
      v
 latch-engine
      |
      v
 local process/filesystem
```

The local machine always initiates the connection. No inbound listening port, port forwarding, public localhost, or arbitrary tunnel is required.

## Why WebSockets + Redis

Vercel currently supports native WebSockets on Functions with Fluid Compute. A WebSocket remains attached to the Function instance that accepted it, while later HTTP requests may execute on another instance. V0.2 therefore uses Redis only as a small coordination layer:

- device presence is an expiring Redis key
- each Function instance subscribes to its own dispatch channel
- an HTTP request publishes to the instance holding the target device socket
- the device response is published by `request_id` so the HTTP instance can resolve the correct pending request

No command or workspace content is persisted by the Router. The in-memory coordinator is used by tests and the local E2E harness only.

## Device identity

Each local installation has:

- `DeviceId`: generated once as a UUID and persisted locally
- `DeviceName`: supplied by `LATCH_DEVICE_NAME`

The default identity file is obtained from the platform local application-data directory. On Windows it is normally:

```text
%LOCALAPPDATA%\Latch\device.json
```

Identity data is not stored in a Latch project workspace. `LATCH_DEVICE_ID_PATH` exists only as an override for tests or controlled installations.

## Pairing and control authentication

V0.2 deliberately has no user accounts or OAuth.

Two independent cryptographically strong secrets are configured on the Router:

- `LATCH_PAIRING_TOKEN`: authenticates a local device WebSocket hello
- `LATCH_CONTROL_TOKEN`: authenticates `/api/devices` and remote request calls

Generate each token independently:

```bash
cd router
npm run token
```

The generator uses 32 random bytes encoded as base64url. Tokens are compared through fixed-length SHA-256 digests with a timing-safe comparison. Token values are never included in normal Router logs or returned errors.

Pairing currently authorizes the connected device to accept the existing Latch protocol from this Router. This is a development proof, not the final end-user permission model.

## Router environment

Required production variables:

```text
LATCH_PAIRING_TOKEN=<secret>
LATCH_CONTROL_TOKEN=<different-secret>
LATCH_REDIS_URL=<TLS Redis connection string>
```

`LATCH_REDIS_URL` must be a native Redis protocol URL (such as the Upstash Redis
TLS endpoint), not an HTTP/REST endpoint. Startup failures return structured
`router_unavailable` responses and are retried by a later request without a
background reconnect loop.

Optional:

```text
LATCH_REQUEST_TIMEOUT_MS=30000
```

The Router fails closed with HTTP 503 if required configuration is absent.

## Connection lifecycle

`latch-link` converts `https://...` to `wss://.../api/link`, authenticates with its persisted identity, and then receives typed remote envelopes.

On disconnect it automatically retries with bounded exponential backoff:

```text
1s -> 2s -> 4s -> 8s -> 16s -> 30s -> 30s ...
```

A successful authenticated connection resets the backoff to 1 second. WebSocket ping/pong remains responsive while local execution runs on Tokio's blocking pool. Vercel may recycle long-lived Function connections, so reconnect is normal behavior.

Presence expires after 90 seconds if an instance disappears without a clean disconnect. Normal disconnect removes presence immediately when possible.

## Request routing and correlation

The Router wraps, but does not replace, `latch-protocol`.

A control request body is the existing protocol envelope:

```json
{
  "id": "caller-id",
  "version": 1,
  "method": "exec.run",
  "params": {}
}
```

The relay allocates an independent UUID `request_id` for routing. This ID selects the correct pending request across Function instances and device responses. The HTTP response is the original typed Latch `ResponseEnvelope`; the relay ID is also returned as `x-latch-request-id`.

Pending requests have a timeout and are removed on response, timeout, or device disconnect. Concurrent responses are correlated independently.

## API

### Health

```text
GET /api/health
```

Public and contains no sensitive data.

### Devices

```text
GET /api/devices
Authorization: Bearer <LATCH_CONTROL_TOKEN>
```

Returns currently online devices.

### Send a Latch request

```text
POST /api/devices/{deviceId}/request
Authorization: Bearer <LATCH_CONTROL_TOKEN>
Content-Type: application/json
```

The body is an existing Latch protocol request envelope. The Router validates only the generic envelope shape and does not maintain a duplicate method schema.

## Local startup

On the Windows machine that should receive requests:

```powershell
$env:LATCH_ROUTER_URL="https://<deployment>.vercel.app"
$env:LATCH_PAIRING_TOKEN="<same pairing token configured on Vercel>"
$env:LATCH_DEVICE_NAME=$env:COMPUTERNAME
cargo run -p latch-link
```

The connection is outbound TLS only.

Development secrets may be kept outside the repository in
`%LOCALAPPDATA%\Latch\secrets.ps1` and loaded into the current PowerShell process:

```powershell
. "$env:LOCALAPPDATA\Latch\secrets.ps1"
```

The file should be readable only by the current Windows user, administrators,
and `SYSTEM`. Never commit or print its contents.

## Manual acceptance test

With `latch-link` running, use the Router control token from a trusted terminal.

1. Find the connected `device_id`:

```powershell
$headers = @{ Authorization = "Bearer <control-token>" }
Invoke-RestMethod -Headers $headers -Uri "https://<deployment>.vercel.app/api/devices"
```

2. Open a local workspace. Replace the path with a directory you intentionally authorize for this proof:

```powershell
$body = @{
  id = "open-1"
  version = 1
  method = "workspace.open"
  params = @{ path = "D:\Programming\03_Projects\personal-projects\Latch" }
} | ConvertTo-Json -Depth 6

$open = Invoke-RestMethod -Method Post -Headers $headers -ContentType "application/json" -Body $body -Uri "https://<deployment>.vercel.app/api/devices/<device-id>/request"
$workspaceId = $open.result.data.workspace_id
```

3. Execute the real local Node binary:

```powershell
$body = @{
  id = "node-version-1"
  version = 1
  method = "exec.run"
  params = @{
    workspace_id = $workspaceId
    program = "node"
    args = @("--version")
  }
} | ConvertTo-Json -Depth 6

Invoke-RestMethod -Method Post -Headers $headers -ContentType "application/json" -Body $body -Uri "https://<deployment>.vercel.app/api/devices/<device-id>/request"
```

Acceptance requires an `ok` Latch response with exit code `0` and the actual Node version from that Windows machine. Do not treat a mocked/local CI echo as the deployed-machine acceptance.

The same production proof and the V0.1 filesystem/process smoke suite are
repeatable from the repository root after loading the local secrets:

```powershell
. "$env:LOCALAPPDATA\Latch\secrets.ps1"
.\scripts\accept-v02.ps1 -FullSmoke
```

On Windows, the engine retains the opened workspace handle until `latch-link`
exits. The script deletes all test files immediately; if the now-empty temporary
directory remains locked, stop `latch-link` before removing it.

## Security boundaries

The V0.1 filesystem capability boundary is unchanged. Relative filesystem operations remain confined to an explicitly opened workspace and still reject traversal/symlink escapes.

However:

```text
filesystem permission != command permission
```

Remote pairing/control authentication authorizes invocation of the existing protocol, including workspace opening and command execution. Commands inherit the Windows/Linux/macOS permissions of the local user and are **not** filesystem-sandboxed by Latch. A command may access resources that the OS user can access even when `latch-fs` would reject the same path.

V0.2 is therefore a transport proof, not a final remote-permission or consent system.

## Vercel deployment

The Vercel project root is `router/`. Fluid Compute is enabled and the WebSocket-capable Router Function has a 300-second maximum duration for Hobby compatibility.

Before production deployment, configure the three required environment variables above. The repository intentionally contains no secrets.

## Known V0.2 limits

- one shared pairing token and one shared control token; no user accounts/OAuth
- Redis is used for ephemeral relay coordination; the current device listing uses a small proof-scale key scan
- presence is not durable history; an offline device disappears from `/api/devices`
- no per-method approval or interactive local consent yet
- an HTTP relay request may time out while an already-authorized local `exec.run` continues until its own local timeout
- local Engine state is in-memory; restarting `latch-link` loses open workspace/process IDs
- no ChatGPT Plugin/App integration yet
