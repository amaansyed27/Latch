# Latch for ChatGPT

**Latch for ChatGPT — Your local machine, inside ChatGPT.**

Latch is a secure bridge foundation for letting a remote control plane invoke typed operations on a user's own machine. V0.2 adds the **Remote Router Proof**: the existing V0.1 local engine remains the execution environment, while a Vercel Router relays authenticated requests to an outbound-only local connection.

The ChatGPT Plugin/App is **not** part of V0.2.

## V0.2 architecture

```text
future ChatGPT Plugin
        |
        v
   Latch Router (Vercel)
        |
        | authenticated relay
        v
     latch-link
   outbound WebSocket
        |
        v
    latch-engine
     /       \
latch-fs   latch-exec
```

`latch-engine` is the reusable V0.1 composition layer. `latch-daemon` still exposes the original newline-delimited stdin/stdout transport; `latch-link` is a second transport adapter and does not duplicate filesystem or process logic.

The deployed Router uses Vercel WebSockets plus Redis for ephemeral presence and cross-instance request correlation. The Router never executes user commands.

## Local capabilities

- capability-scoped workspace filesystem operations
- bounded/timeout-protected `exec.run`
- managed process start/status/output/kill and shutdown cleanup
- versioned `latch-protocol` v1
- stdin/stdout local daemon
- outbound-only Router connection with persisted `DeviceId`
- automatic reconnect with bounded exponential backoff

Command execution is **not** an OS sandbox. Commands start in their Latch workspace but inherit the permissions of the user running Latch.

## Build and test

Rust:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace
```

Router:

```bash
cd router
npm install
npm run lint
npm run typecheck
npm test
npm run build
```

The cross-language local relay proof is run in CI after building both sides:

```bash
node router/scripts/e2e-local.mjs
```

## Start Latch Link

Configure the deployed Router URL, its pairing token, and a human-readable device name. On Windows PowerShell:

```powershell
$env:LATCH_ROUTER_URL="https://<your-latch-router>.vercel.app"
$env:LATCH_PAIRING_TOKEN="<pairing-token>"
$env:LATCH_DEVICE_NAME=$env:COMPUTERNAME
cargo run -p latch-link
```

The generated `DeviceId` is persisted outside project workspaces. On Windows the default is `%LOCALAPPDATA%\Latch\device.json`.

See [`docs/router.md`](docs/router.md) for Router setup, deployment, pairing, APIs, and the manual `node --version` acceptance test. See [`docs/architecture.md`](docs/architecture.md) for local security boundaries and dependency direction.
