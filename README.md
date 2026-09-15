# Latch

**Your local machine, inside ChatGPT.**

Latch lets ChatGPT, Codex, and compatible MCP clients use explicit tools on a computer you own. Your computer makes an outbound encrypted connection; the Router never runs your commands.

> **Private beta:** V0.4.5 is for personal testing. Commands are powerful and the Windows installer is currently unsigned.

## Install on Windows

1. Visit [latch-router.vercel.app/download](https://latch-router.vercel.app/download).
2. Download and run `LatchSetup-x64.msi` on Windows 10 or 11 (x64).
3. Open **Latch** from the Start menu.
4. Sign in on the Latch website, choose **Add computer**, generate a pairing code, and paste it into the Latch app.

Latch installs per user under `%LOCALAPPDATA%\Programs\Latch`, lives in the Windows system tray, and starts automatically after sign-in. Closing the Latch window hides it to the tray; **Quit Latch** explicitly disconnects it.

No Rust, Cargo, Git, administrator setup, or environment variables are required for normal use. Windows SmartScreen may warn because the private-beta installer is not signed with a publicly trusted certificate.

### Tray app

The tray app shows the current connection state and provides quick access to:

- Latch dashboard and device management
- restart connection
- local diagnostics
- bounded local logs
- quit/disconnect

The CLI remains installed as a fallback and developer interface:

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

Connect a supported MCP client to `https://latch-router.vercel.app/mcp`, authorize through OAuth, then ask it to list your computers, open a workspace, read a file, or run a command. The current beta deliberately exposes only:

- `latch_devices_list`
- `latch_workspace_open`
- `latch_file_read`
- `latch_exec_run`

## Local data and logs

- Device identity: `%LOCALAPPDATA%\Latch\device.json`
- Device credential: Windows Credential Manager
- Connection status: `%LOCALAPPDATA%\Latch\status.json`
- Bounded logs: `%LOCALAPPDATA%\Latch\logs\current.log` and `previous.log`

Uninstall removes application binaries, PATH integration, Start menu/startup shortcuts, and the tray application. It preserves the DeviceId and credential by default so reinstalling can retain pairing.

## Security boundary

Filesystem tools are confined to an opened workspace and reject absolute paths, parent traversal, and symlink or junction escapes.

Command execution is **not sandboxed**. Commands run with the permissions of the logged-in Windows user running Latch and may access anything that user can access. Latch is intentionally per-user and never runs as a SYSTEM service.

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

The release workflow publishes `LatchSetup-x64.msi`, a portable ZIP containing the tray app and CLI, and SHA-256 checksums when a version tag is pushed.

## Architecture

```text
ChatGPT / Codex / MCP client
        | HTTPS + OAuth
        v
Latch Router + ephemeral Redis routing
        | outbound TLS WebSocket
        v
Latch tray app -> latch-link -> latch-engine -> local filesystem/processes
```

See [docs/architecture.md](docs/architecture.md) and [docs/router.md](docs/router.md) for implementation details.
