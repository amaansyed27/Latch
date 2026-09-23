# Latch

**A Windows-native AI agent runtime for normal ChatGPT conversations.**

Latch gives an authenticated ChatGPT/MCP conversation controlled access to a Windows PC through a small public tool surface while keeping authority, resources, permissions, approvals, secrets, and verification on the device.

> **Private beta:** `v0.6.0-beta.1` is powerful software. Interactive terminals execute with the logged-in Windows user's authority, and the private-beta installer is currently unsigned.

## What changed in V0.6

V0.6 moves beyond the V0.5 files/terminal bridge into a coordinated Windows agent runtime:

- explicit cross-surface Latch sessions
- semantic Windows control through Windows UI Automation
- persistent interactive terminals backed by ConPTY on Windows
- persistent Playwright browser contexts/tabs with DOM/accessibility, console, network, downloads, and screenshots
- Observe → Act → Verify result contracts for mutating actions
- persistent local MCP federation with lazy search → describe → call
- native application, clipboard, and audio providers
- bounded local event streams with cursors and long-waits
- deterministic capability routing rather than an extra local LLM
- fine-grained local `deny / ask / allow` permission policy and approval queue

The intended engineering distinction is coordination across control surfaces: files, terminals, native Windows semantics, browser semantics/devtools, local MCP tools, raw computer fallback, events, and deterministic verification share one device-local task/session model.

## Install on Windows

1. Download the private-beta MSI from the Latch download page.
2. Run `LatchSetup-x64.msi` on Windows 10/11 x64.
3. Open **Latch** from the Start menu.
4. Sign in on the Latch website, choose **Add computer**, generate a pairing code, and paste it into Latch Desktop.
5. Approve folders and select the capabilities ChatGPT may use.

Latch installs per user under `%LOCALAPPDATA%\Programs\Latch`, starts after sign-in, and stays in the Windows tray. It does not install a SYSTEM service or silently elevate itself.

The V0.6 package also includes the pinned browser runtime required by `latch_browser`: Node, Playwright `1.63.0`, and its pinned Chromium payload. Released builds do not execute `npx ...@latest` on the user's machine.

## Public MCP surface

V0.6 intentionally exposes nine stable domain tools rather than dozens of primitive operations:

1. `latch_devices`
2. `latch_session`
3. `latch_inspect`
4. `latch_files`
5. `latch_exec`
6. `latch_act`
7. `latch_browser`
8. `latch_tools`
9. `latch_events`

Each domain tool uses bounded tagged operations. Public MCP schemas and the internal protocol are separate: the device protocol is strongly typed and retains a compatibility path for V0.5-era requests, while new functionality routes through protocol v3 `agent` domains.

Connect an MCP client to:

```text
https://latch-router.vercel.app/mcp
```

Authorization uses OAuth capability scopes. OAuth is necessary but not sufficient: device-local permission policy, Pause, approved roots, and device ownership remain authoritative.

## Runtime architecture

```text
Normal ChatGPT / MCP client
            |
        MCP + OAuth
            v
       Latch Router
       thin relay/control plane
            |
   outbound TLS WebSocket
            v
        latch-link
            |
            v
  Windows Agent Runtime
      latch-engine
      |-- Session Manager
      |-- Capability Registry / Resolver
      |-- Observe -> Act -> Verify
      |-- Event Bus
      |-- Permission Enforcement
      |
      |-- latch-fs
      |-- latch-exec
      |-- latch-terminal
      |-- latch-windows (apps/UIA/clipboard/audio)
      |-- latch-browser (Playwright)
      |-- latch-mcp-client federation manager
      `-- latch-computer raw screenshot/input fallback
```

ChatGPT decides what should happen. Latch deterministically chooses and enforces the strongest appropriate local capability, performs known retries/fallbacks, tracks resources, and verifies provider-known postconditions. Latch does **not** contain a second LLM planner.

The outbound link no longer serializes all expensive work behind one engine mutex. Terminal output can continue while filesystem, browser, UIA, and MCP work occurs; synchronization is scoped to individual managers/resources.

## Sessions and events

`latch_session` creates explicit model-visible task sessions. A session can bind workspaces, terminals, browser contexts/tabs, UI references, selected MCP providers, revisions, and an event cursor.

Raw OS handles are not treated as durable. If the Latch worker restarts, recoverable metadata may be restored but terminals are lost and UI references become stale. The session is marked degraded rather than silently pretending its resources survived.

`latch_events` exposes bounded/coalesced events after a cursor and supports a long-wait. Latch never emits one remote event per terminal byte or UI property mutation.

## Windows semantic control

`latch_inspect` and `latch_act` prefer Windows UI Automation over pixels when a standard desktop control is available. Semantic elements expose bounded fields such as role/control type, name, value where appropriate, automation ID, bounds, focus/enabled/offscreen state, and normalized supported actions.

UI references are opaque and session-scoped. COM/UIA access runs on a dedicated worker thread rather than the Tauri UI thread. Secure desktop/UAC surfaces are rejected, and integrity/UIPI boundaries are respected. Latch does not bypass an elevated target, silently elevate, or use a weaker input path to evade a denied semantic capability.

Desktop fallback order is conceptually:

```text
UIA semantic action
→ semantic bounds/context
→ screenshot/vision
→ raw mouse/keyboard
```

A permission denial terminates that route.

## Persistent terminals

`latch_exec` retains ordinary one-shot execution and also manages persistent interactive terminals. Windows terminals use ConPTY through the local PTY backend and support create, write, bounded read/snapshot, resize, interrupt, kill, and list.

Latch discovers PowerShell 7, Windows PowerShell, cmd, Git Bash, and WSL distributions when available instead of assuming they are installed. Terminal output is bounded and cursor-based rather than repeatedly returning a giant ANSI transcript.

**Security truth:** allowing a terminal allows arbitrary shell commands with the logged-in user's authority. Turning off Latch's Clipboard provider does not magically prevent an unrestricted shell from invoking Windows clipboard APIs. Capability switches govern Latch providers; they are not an OS sandbox.

## Browser runtime

`latch_browser` uses a persistent pinned Playwright provider. It supports isolated/testing contexts and a Latch persistent profile, persistent tab IDs, navigation, bounded semantic/ARIA state, semantic find/act, console/network/download cursors, page state, and screenshots.

Authenticated control of an already logged-in user's default Chrome profile is intentionally **not** claimed. That remains a separately permissioned capability requiring an explicit supported integration; V0.6 does not use unsafe default-profile hacks.

## Local MCP federation

Local MCP providers are managed persistently where useful. Stdio children remain alive instead of reconnecting for every operation, HTTP transports reuse connections where supported, and catalogues are cached/versioned locally.

Third-party tool schemas are not inserted wholesale into ChatGPT's initial context. `latch_tools` does:

```text
search (max ~5 concise candidates)
→ describe one selected tool
→ call it by opaque tool ref
```

Local commands, endpoint configuration, environment references, and secret values stay on the PC unless a selected tool itself explicitly returns data. Latch never assumes arbitrary third-party MCP semantics for automatic routing unless there is a built-in adapter or explicit mapping.

## Local permissions and approvals

The Desktop app exposes explicit `deny / ask / allow` policy for:

- files read / files write
- one-shot execution / interactive terminal
- application control
- UI inspection / UI control
- screen capture / raw input
- isolated browser / authenticated browser
- clipboard read / clipboard write
- MCP discovery / MCP execution
- native system control

Presets (`Observe`, `Work`, `Developer`, `Full Control`) only populate these explicit values. They do not replace the underlying policy.

Ask-mode operations enter a local approval queue and can be denied, allowed once, or allowed for the current session. Permission denial never causes the resolver to fall through to a weaker interface that would bypass policy.

## Filesystem boundary

Filesystem operations remain confined to locally approved roots and opaque workspace IDs. Remote clients do not choose arbitrary absolute paths during normal operation. Latch rejects absolute paths, parent traversal, and workspace escapes.

## Router boundary

The Router authenticates users/devices and relays bounded protocol messages. Device state, file contents, terminal logs, screenshots, browser state, MCP secrets, UI handles, and approval decisions remain device-local. Redis is used for presence/coordination rather than as durable agent memory.

Treat file contents, browser pages, terminal output, screenshots, and third-party MCP output as **untrusted data**. They may contain prompt-injection text; Latch returns the data but does not promote it into trusted instructions.

See [docs/security.md](docs/security.md) and [docs/windows-agent-runtime.md](docs/windows-agent-runtime.md).

## Validation status

Automated CI is responsible for:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace`
- Router install, lint, typecheck, tests, build, and Playwright web tests
- Router → local engine E2E
- public MCP → local MCP relay E2E
- synthetic ~250-tool MCP catalogue scale behavior
- Windows workspace tests/build
- MSI version/payload checks
- bundled Node/Playwright/Chromium validation
- clean MSI install/uninstall smoke test

Hosted CI does **not** pretend it proved an interactive Windows desktop. Before promoting the beta, run the physical-machine checklist in [docs/physical-acceptance-v0.6.md](docs/physical-acceptance-v0.6.md), including Notepad/UIA, persistent terminal, browser fixture, native volume/clipboard/Calculator, and the verified developer loop.

## Build from source

Developers need the Rust toolchain and Node.js 22+:

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
npm run test:web
```

Build the Windows MSI with WiX 5.0.2:

```powershell
dotnet tool install --global wix --version 5.0.2
.\scripts\build-windows-installer.ps1
```

Application version: `0.6.0-beta.1`  
MSI numeric ProductVersion: `0.6.0`

## Release safety

`v0.6.0-beta.1` may only be published by the manual Windows release workflow from the exact current `main` SHA after that SHA has a successful push CI run. The release workflow reruns static/tests, rebuilds the package, validates MSI/browser payloads and checksums, creates a prerelease, and verifies the tag points to the exact tested SHA.

## Local data

- Local configuration / approved roots / capability policy: `%LOCALAPPDATA%\Latch\local-config.json`
- Device identity: `%LOCALAPPDATA%\Latch\device.json`
- Device credential: Windows Credential Manager
- Connection status: `%LOCALAPPDATA%\Latch\status.json`
- Bounded local activity metadata: `%LOCALAPPDATA%\Latch\activity.json`
- Bounded logs: `%LOCALAPPDATA%\Latch\logs\current.log` and `previous.log`

## License

Latch is licensed under the [MIT License](LICENSE).
