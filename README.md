# Latch

**Your local machine, inside ChatGPT.**

Latch lets ChatGPT, Codex, and compatible MCP clients use explicit tools on a computer you own. The computer makes an outbound encrypted connection; the Router never runs your commands.

> **Private beta:** V0.4.2 is for personal dogfooding. Commands are powerful and the Windows installer is currently unsigned.

## Install on Windows

1. Visit [latch-router.vercel.app/download](https://latch-router.vercel.app/download).
2. Download and run `LatchSetup-x64.msi` on Windows 10 or 11 (x64).
3. Open a new Terminal and verify:

```powershell
latch --version
```

Latch installs per user under `%LOCALAPPDATA%\Programs\Latch`, adds `latch` to the user PATH, and starts invisibly after login. Rust, Cargo, Git, administrator access, and environment variables are not required.

Windows SmartScreen may warn because the private-beta installer is not signed with a publicly trusted certificate.

## Pair a computer

1. Sign in at [Latch Devices](https://latch-router.vercel.app/devices).
2. Choose **Create pairing code**.
3. In Terminal, run the displayed command:

```powershell
latch pair <code>
```

The one-time code is exchanged for a unique device credential stored in Windows Credential Manager. The credential is never printed. Pairing starts Latch in the background automatically.

Useful commands:

```text
latch status
latch start
latch stop
latch restart
latch doctor
latch reset
```

`latch reset` asks for confirmation, stops Latch, and removes the local DeviceId and credential. It does not silently revoke the cloud device; revoke that separately on the Devices page.

## Use with ChatGPT or Codex

Connect a supported MCP client to `https://latch-router.vercel.app/mcp`, authorize through OAuth, then ask it to list your computers, open a workspace, read a file, or run a command. V0.4.2 deliberately exposes only:

- `latch_devices_list`
- `latch_workspace_open`
- `latch_file_read`
- `latch_exec_run`

## Local data and logs

- Device identity: `%LOCALAPPDATA%\Latch\device.json`
- Device credential: Windows Credential Manager
- Connection status: `%LOCALAPPDATA%\Latch\status.json`
- Bounded logs: `%LOCALAPPDATA%\Latch\logs\current.log` and `previous.log`

Uninstall removes the application, PATH entry, and login startup entry. It preserves the DeviceId and credential so reinstalling can retain pairing.

## Security boundary

Filesystem tools are confined to an opened workspace and reject absolute paths, parent traversal, and symlink or junction escapes.

Command execution is **not sandboxed**. Commands run with the permissions of the logged-in Windows user running Latch and may access anything that user can access. Latch is intentionally a per-user background application, never a SYSTEM service.

See [Security](https://latch-router.vercel.app/security) and [docs/public-auth-v0.4.md](docs/public-auth-v0.4.md).

## Build from source

Developers need the Rust toolchain and Node.js 22 or later:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace

cd router
npm install
npm run lint
npm run typecheck
npm test
npm run build
```

Build the Windows MSI with WiX 4:

```powershell
dotnet tool install --global wix --version 4.0.6
.\scripts\build-windows-installer.ps1
```

The release workflow publishes `LatchSetup-x64.msi`, a portable ZIP, and SHA-256 checksums when a version tag is pushed.

## Architecture

```text
ChatGPT / Codex / MCP client
        | HTTPS + OAuth
        v
Latch Router + ephemeral Redis routing
        | outbound TLS WebSocket
        v
Latch Link -> latch-engine -> local filesystem/processes
```

See [docs/architecture.md](docs/architecture.md) and [docs/router.md](docs/router.md) for implementation details.
