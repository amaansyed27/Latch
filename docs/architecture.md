# Latch Architecture — V0.6

## Principle

> ChatGPT decides what should happen. Latch decides the strongest deterministic way to make it happen.

Latch does not run a second planning LLM. It performs capability selection, permission enforcement, resource/session state, known retries/fallbacks, event coordination, normalization, and deterministic verification.

## Topology

```text
ChatGPT / MCP client
        |
    MCP + OAuth
        v
Latch Router
(thin relay/control plane)
        |
 Redis presence / request coordination
        |
outbound TLS WebSocket
        v
     latch-link
        |
        v
Windows Agent Runtime (latch-engine)
   |-- Session Manager
   |-- Capability Registry / Resolver
   |-- Observe -> Act -> Verify
   |-- Event Bus
   |-- Permission Enforcement
   |-- latch-fs
   |-- latch-exec
   |-- latch-terminal
   |-- latch-windows
   |-- latch-browser
   |-- latch-mcp-client federation
   `-- latch-computer raw fallback
```

The user's device initiates the only persistent connection. No inbound public port is required.

## Public MCP vs internal protocol

The public ChatGPT-facing surface is deliberately small:

- `latch_devices`
- `latch_session`
- `latch_inspect`
- `latch_files`
- `latch_exec`
- `latch_act`
- `latch_browser`
- `latch_tools`
- `latch_events`

Each tool uses a bounded operation discriminator. Internally protocol v3 transports typed `agent` domains and strongly typed IDs. The V0.5 request variants remain as a compatibility path but are not re-expanded into dozens of public MCP tools.

## Concurrency

V0.6 removes the global mutable-engine bottleneck. `latch-link` can dispatch independent requests concurrently into `Arc<Engine>`. Expensive resources synchronize only where needed:

- workspace registry / individual filesystem calls
- managed processes
- terminal manager / individual PTYs
- Windows UIA COM worker
- browser provider process
- persistent MCP manager
- sessions
- event bus
- raw computer fallback

A terminal can keep producing output while browser, filesystem, UIA, or MCP work is occurring.

## Sessions

Latch sessions are device-local task state and do not depend on MCP transport sessions. A session can reference:

- selected device/workspaces
- terminal IDs
- browser context/tab IDs
- app/window/UI references
- selected MCP providers/tools
- observation revision
- event cursor
- timestamps and lifecycle state

Raw OS handles are not persisted as if durable. After a worker restart, recoverable metadata can be restored, terminal handles are lost, UI references are stale, and the session becomes degraded. Resource TTL/cleanup prevents abandoned task state from accumulating indefinitely.

## Capability resolver

Providers advertise semantic operation, availability, permission requirements, cost/priority, verification support, and required session state. Ranking is operation-specific, not one universal priority list.

Examples:

```text
web click       -> Playwright > UIA > pixels
activate Chrome -> Win32/native > UIA > mouse
mapped Blender  -> explicit Blender MCP adapter > UIA > pixels
desktop control -> UIA > screenshot/vision > raw input
```

A denied capability cannot be bypassed by falling through to a weaker route.

## Observe → Act → Verify

Mutating actions report an action identity, deterministic route used, outcome, verification result, changed references where available, and observation revision.

Supported outcome classes include:

- `verified`
- `applied_unverified`
- `no_change`
- `failed`
- `verification_failed`
- `permission_denied`

`auto` verification uses only provider-known postconditions. `required` fails if verification cannot pass. `none` records the action without claiming goal success. Arbitrary goal-level judgement remains with ChatGPT.

Examples:

- file write → hash/readback
- app launch → process/window exists
- UIA set-value → value readback
- browser action → supplied/provider-known DOM or navigation condition

## Windows semantics

`latch-windows` owns cohesive Windows-native semantics rather than unrelated platform hacks:

- application discovery/launch/activation/quit/open-target
- Windows UI Automation
- clipboard
- audio/volume

UIA runs on a dedicated COM worker. Remote element references are opaque, session-scoped IDs, never COM pointers. Queries are depth/result bounded and cache only the properties needed to produce compact semantic elements.

Integrity boundaries are fail-closed. Secure desktop/UAC is not automated. Elevated targets return a clear boundary error rather than triggering privilege escalation or input tricks.

## Terminals

`latch-terminal` manages persistent interactive PTYs. On Windows the backend is ConPTY. Each terminal owns its child shell, input, bounded output ring, normalized logical snapshot, event/sequence cursor, and Windows Job Object. PowerShell 7, Windows PowerShell, cmd, Git Bash, and WSL profiles are discovered rather than assumed.

Terminals are intentionally not an OS sandbox.

## Browser

`latch-browser` owns a persistent provider process backed by pinned Playwright `1.63.0`. Browser context and tab IDs survive individual MCP calls. The primary observation path is semantic DOM/accessibility state; screenshots are explicit and secondary.

Supported profiles:

1. isolated/testing context
2. Latch persistent profile

Authenticated control of an existing default Chrome profile is designed as a separate permission and future explicit integration; V0.6 does not fake support or copy/attach to a locked default profile.

## MCP federation

`latch-mcp-client` maintains useful stdio children and reusable HTTP services, caches catalogues, and exposes opaque provider/tool refs. The initial public Latch MCP context does not contain every third-party schema.

Discovery is lazy:

```text
search -> <=5 candidates
       -> describe selected schema
       -> call selected opaque ref
```

Provider command, endpoint, environment mapping, and secret values remain local. Automatic resolver integration is limited to built-in adapters or explicit mappings; arbitrary third-party semantics are never guessed.

## Events

The local event bus uses a common bounded envelope containing sequence, timestamp, session, source, event type, summary, and bounded payload/ref. Producers coalesce aggressively. Consumers use an `after_sequence` cursor and optional long-wait instead of repeatedly polling full state.

## Permissions

Local capabilities use `deny / ask / allow`. OAuth scope grants remain an outer remote authorization layer; local policy remains authoritative.

Ask-mode requests are queued in Latch Desktop and support deny, allow once, or allow for session. Session grants are local and ephemeral.

## Filesystem

The approved-root model remains unchanged in principle. Roots are approved locally and exposed remotely only as opaque IDs/display names. Workspace-relative paths are canonicalized/confined through `latch-fs`. Absolute remote paths, traversal, and escapes fail closed.

## Router persistence boundary

Router/Redis may persist account/device authorization metadata and transient presence/coordination data. It must not become durable storage for file contents, terminal logs, screenshots, browser state, UI handles, local MCP secrets, or approval decisions.

## Crate boundaries

- `latch-core` — shared typed IDs/domain primitives
- `latch-local` — approved roots, deny/ask/allow policy, approvals, pause, MCP config, local metadata
- `latch-fs` — capability-confined workspace filesystem
- `latch-exec` — one-shot/managed processes
- `latch-terminal` — persistent PTY/ConPTY sessions
- `latch-windows` — apps, UIA, clipboard, audio
- `latch-browser` — persistent Playwright provider
- `latch-computer` — explicit screenshot/raw mouse-keyboard fallback
- `latch-mcp-client` — persistent local MCP federation
- `latch-protocol` — versioned typed wire DTOs
- `latch-engine` — sessions, resolver, permissions, verification, events, provider composition
- `latch-link` — outbound authenticated WebSocket lifecycle and concurrent dispatch
- `latch-desktop` — local authority/control-center UI
- `latch-daemon` — compatibility local transport
- `router/` — OAuth/MCP/ownership/routing control plane

## Trust model

File contents, terminal output, browser pages, screenshots, and MCP output are untrusted observations. They can contain prompt-injection text. Latch does not interpret arbitrary content as trusted policy or local instructions; ChatGPT receives it as tool data and remains responsible for reasoning about the user's goal.
