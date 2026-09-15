# Latch

**Your local machine, inside ChatGPT.**

Latch gives ChatGPT, Codex, and compatible MCP clients explicit access to a computer you own. The computer keeps an outbound encrypted connection to the Latch Router; local commands, files, screen capture, computer control, and local MCP integrations execute on your machine rather than in the Router.

> **Private beta:** V0.5 is intended for personal testing. Command execution and computer control are powerful, and the Windows installer is currently unsigned.

## Install on Windows

1. Visit [latch-router.vercel.app/download](https://latch-router.vercel.app/download).
2. Download and run `LatchSetup-x64.msi` on Windows 10 or 11 (x64).
3. Open **Latch** from the Start menu.
4. Sign in on the Latch website, choose **Add computer**, generate a pairing code, and paste it into Latch Desktop.
5. In the desktop app, approve the folders and capabilities you want Latch to expose.

Latch installs per user under `%LOCALAPPDATA%\Programs\Latch`, lives in the Windows system tray, and starts automatically after sign-in. Closing the window keeps Latch running in the tray. **Quit Latch** disconnects it.

No Rust, Cargo, Git, administrator setup, or environment variables are required for normal use. Windows SmartScreen may warn because the private-beta installer is not signed with a publicly trusted certificate.

## What V0.5 can do

Latch V0.5 exposes a bounded MCP tool surface for:

- listing connected computers
- listing locally approved folders and opening an approved workspace
- listing, inspecting, reading, searching, writing, patching, moving, creating, and deleting workspace files
- running commands and managing long-running processes with stdin, polling, and termination
- listing displays and windows, taking an explicit screenshot, focusing windows, and sending mouse/keyboard input when locally enabled
- discovering locally configured MCP integrations and calling their tools through Latch when explicitly allowed

The model never chooses a raw absolute folder path during normal remote use. The user approves roots locally, and remote clients receive opaque root/workspace IDs plus display names.

## Desktop controls

The Windows desktop app is the local authority for remote access. It provides:

- **Folders** — add/remove approved roots with a native folder picker
- **Local MCPs** — configure, enable, test, and remove local stdio or HTTP MCP integrations
- **Permissions** — independently enable Files, Commands, Screen, Computer control, MCP discovery, and MCP execution
- **Activity** — bounded local metadata for recent Latch actions
- **Pause remote access** — immediately rejects new remote operations without deleting pairing
- **Diagnostics** — local connection/setup checks and bounded logs

Local MCP environment configuration stores environment-variable names, not secret values. Secret values are resolved only on the local computer when an integration starts.

The tray menu also provides quick access to Latch, device management, restart, pause/resume, and quit.

## Use with ChatGPT or Codex

Connect a supported MCP client to:

```text
https://latch-router.vercel.app/mcp
```

Authorization uses OAuth. Requested scopes are separated by capability (devices, roots, files, commands, screen, computer control, and local MCP access), while the desktop permission switches remain an additional local deny layer.

A typical flow is:

```text
list computers
→ list approved folders
→ open an approved workspace
→ read/edit files or run a command
```

Computer-use and local-MCP tools are available only when both OAuth authorization and the corresponding local permission allow them.

## Validation status

Verified in automated V0.5 testing:

- stdio MCP client support
- Streamable HTTP MCP client support
- local MCP server discovery
- local MCP tool discovery
- local MCP tool calls
- Router → `latch-link` → `latch-engine` relay behavior
- public MCP → local MCP relay behavior
- Windows test/build/MSI packaging

Supported through the generic MCP bridge but **not yet manually verified against the real third-party integration** in this beta:

- Blender MCP
- Playwright/browser MCP

The release is not blocked on those third-party manual checks because they do not require a separate Latch subsystem; they use the same generic local MCP bridge.

## Security boundary

Filesystem operations are confined to an opened workspace rooted in a folder approved locally. Latch rejects absolute remote paths, parent traversal, and filesystem escapes.

Command execution is **not sandboxed**. Commands run with the permissions of the logged-in Windows user running Latch and can access anything that account can access. Latch is intentionally per-user and never runs as a SYSTEM service.

Screen capture is performed only when the screenshot tool is explicitly called. Screenshot bytes are relayed in memory as MCP image content and are not intentionally persisted by the Router.

Local MCP tools execute on the user's computer. Their output, like file contents, command output, and screenshots, must be treated as untrusted data.

Revoking a device or disabling a local permission takes precedence over a previously granted OAuth scope.

See [Security](https://latch-router.vercel.app/security), [docs/architecture.md](docs/architecture.md), and [docs/router.md](docs/router.md).

## Local data and logs

- Local configuration and approved roots: `%LOCALAPPDATA%\Latch\local-config.json`
- Device identity: `%LOCALAPPDATA%\Latch\device.json`
- Device credential: Windows Credential Manager
- Connection status: `%LOCALAPPDATA%\Latch\status.json`
- Activity metadata: `%LOCALAPPDATA%\Latch\activity.json`
- Bounded logs: `%LOCALAPPDATA%\Latch\logs\current.log` and `previous.log`

Uninstall removes application binaries, PATH integration, Start menu/startup shortcuts, and the tray application. Pairing state is preserved by default so reinstalling can retain the device identity; revoke a device from the Devices page when you want to invalidate it remotely.

## CLI

The CLI remains installed as a fallback and developer interface:

```text
latch status
latch start
latch stop
latch restart
latch doctor
latch reset
```

`latch reset` stops Latch and removes the local DeviceId and credential after confirmation. Cloud revocation is a separate action on the Devices page.

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

The application/release version is `0.5.0-beta.1`. Windows Installer metadata uses numeric `ProductVersion` `0.5.0`, as required by MSI version rules.

## Architecture

```text
ChatGPT / Codex / MCP client
        | HTTPS + OAuth
        v
Latch Router + routing store
        | outbound TLS WebSocket
        v
Latch Link
        |
        v
Latch Engine
   |       |        |         |
 files   process   computer   local MCPs
```

The Router authenticates and routes requests; the local engine remains responsible for local permissions and execution. See [docs/architecture.md](docs/architecture.md) and [docs/router.md](docs/router.md) for implementation details.

## Release safety

`v0.5.0-beta.1` is published only by the manual Windows release workflow from `main`. The workflow verifies that the selected SHA is the current `main` SHA and already has a successful CI run before rebuilding, testing, validating MSI metadata/payloads, checking checksums, and creating the prerelease from that exact SHA.

## License

Latch is licensed under the [MIT License](LICENSE).
