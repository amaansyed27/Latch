# Security — V0.6 Windows Agent Runtime

Latch is a local-authority system. The Router authenticates and relays; the paired Windows device decides whether a capability may execute.

## Reviewed boundaries

### Filesystem confinement

Remote file operations use opaque approved-root/workspace IDs and workspace-relative paths. Canonicalization and capability-relative filesystem access reject absolute paths, parent traversal, and escapes. Removing an approved root invalidates dependent workspaces. This boundary does not sandbox arbitrary shell commands.

### Interactive terminal authority

An unrestricted terminal runs as the logged-in Windows user. It may access anything that user can access, regardless of whether a narrower Latch provider (for example Clipboard) is denied. The Desktop UI and documentation state this explicitly. Terminal permission is therefore a high-authority grant, not a command sandbox.

### UI Automation / integrity

UIA state lives on a dedicated COM worker. Remote refs are opaque/session-scoped and stale refs are rejected. Latch does not automate secure desktop/UAC, defeat UIPI, silently elevate, or install a SYSTEM service. Elevated targets that are outside the current integrity boundary fail closed.

### Fallback bypass

Provider fallback is capability-aware. A denied semantic provider/capability must not fall through to screenshot/raw input or another weaker interface to accomplish the same prohibited action.

### Browser authentication data

The default browser runtime uses isolated or Latch-owned persistent profiles. V0.6 does not attach to/copy the user's normal Chrome profile. Authenticated existing-browser control is a distinct permission and remains incomplete until an explicit supported integration is physically validated.

### Clipboard sensitivity

Clipboard read and write are separate local capabilities. Clipboard contents are returned only for an authorized request and are not intentionally stored by the Router. Users should treat clipboard read as potentially exposing passwords, tokens, personal data, or transient secrets.

### Local MCP secrets

MCP provider commands, endpoint configuration, environment references, and secret values remain local. Stdio environment settings store references/names rather than secret values where supported. The Router receives only authorized discovery/invocation results, not the local provider launch configuration.

### Tool-schema and MCP-output injection

Third-party MCP schemas are discovered lazily rather than globally injected. Search results are concise and selected schemas are fetched only on describe. Arbitrary MCP output is untrusted tool data and may contain prompt injection or hostile markup/text; it is not trusted as Latch policy.

### Browser/file prompt injection

Files, web pages, terminal output, screenshots, and MCP responses are observations, not instructions to the local runtime. Latch does not run a second model that could autonomously reinterpret such content. ChatGPT remains responsible for reasoning about untrusted observations and the user's goal.

### Session/resource leakage

Sessions own volatile refs/resources and use bounded rings/caches plus cleanup/TTL behavior. Worker restart degrades sessions; it does not claim terminals/UI handles survived. Terminal and browser managers expose explicit close/kill operations and shutdown paths.

### Event leakage

Events are device-local, session-associated, bounded, cursor-based, and coalesced. Event payloads should contain only what is required for the authorized caller; high-volume raw streams are not mirrored byte-for-byte into the Router.

### OAuth and device ownership

The Router verifies OAuth capability scopes and selected-device ownership before relay. Device-local policy is an additional authority layer and can deny a request even when OAuth allows it. Pause and device revocation remain independent controls.

### Router persistence

The Router remains a relay/control plane. It must not durably store file contents, terminal transcripts, screenshots, browser state, UI handles, local MCP secrets, or approval decisions. Redis is used for transient presence/coordination/rate-limit state rather than durable task memory.

## Remaining risk

- Allowing `terminal` or broad one-shot execution effectively grants the logged-in user's shell authority.
- A malicious file/page/MCP result can attempt prompt injection against the remote model even though Latch itself does not elevate that text to policy.
- Browser persistent profiles may accumulate authenticated data if the user explicitly signs into the Latch-owned profile; protect that Windows account accordingly.
- Raw input is inherently less semantic and less verifiable than UIA/DOM control; keep it denied/ask unless needed.
- Pending `ask` approval requests are persisted until the user resolves them; V0.6 does not yet expire unresolved prompts by age. Session grants do expire, but a later beta should add an explicit pending-request TTL.
- Private-beta MSI is unsigned, so authenticity currently relies on the GitHub release origin plus published SHA256 checksums.

## Release requirement

Do not label V0.6 physically validated until `docs/physical-acceptance-v0.6.md` has been run on a real interactive Windows session. Hosted CI covers lower-level behavior, packaging, and non-interactive tests but cannot honestly prove secure-desktop/UIA/session-specific desktop behavior.
