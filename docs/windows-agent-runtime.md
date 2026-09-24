# Windows Agent Runtime — V0.6

## Scope

V0.6 is the first Latch runtime designed to coordinate a real developer workflow across files, terminals, Windows semantics, browser semantics/devtools, local MCP providers, events, and deterministic verification.

It intentionally does **not** contain a local planning model. ChatGPT performs goal interpretation and arbitrary reasoning; Latch owns deterministic execution policy and local authority.

## Runtime managers

`latch-engine` composes independently synchronized managers for:

- explicit sessions and cleanup
- approved workspaces/files
- one-shot and managed processes
- persistent terminals
- Windows applications/UIA/clipboard/audio
- persistent Playwright browser contexts/tabs
- persistent local MCP federation
- raw computer fallback
- approvals/permissions
- bounded event streams

## Semantic references

UIA references, terminal IDs, tab IDs, browser context IDs, action IDs, workspace IDs, and MCP tool refs are opaque identifiers. OS/COM/process handles never cross the Router boundary.

UI references are session-scoped and validated against the live UIA runtime ID. A stale or cross-session ref is rejected.

## Resource recovery

A worker restart is not treated as transparent survival:

- approved workspaces may be reopened from metadata
- persistent browser profile data remains on disk but live tab/context handles must be rediscovered/recreated
- terminals are lost
- UI refs are stale
- window/app state is rediscovered

Sessions become degraded when volatile resources disappear.

## Bounded observations

Every high-volume surface has explicit limits:

- UIA depth/result bounds
- terminal output ring + cursor
- browser semantic snapshot bounds
- console/network/download cursors
- MCP search maximum of approximately five candidates
- bounded event ring and long-wait
- explicit screenshots rather than automatic full-desktop capture

## Provider verification

Latch can verify only deterministic provider postconditions. It never claims the user's complete goal succeeded merely because input was sent.

Examples:

- `set_value` rereads UIA value
- volume set rereads system volume
- file write can hash/read back
- app launch checks process/window state
- browser actions can verify supplied DOM/navigation conditions

## Browser runtime packaging

The Windows package contains the runtime dependencies required for `latch_browser`: a Node executable, the bridge process, exact Playwright `1.63.0`, and the browser payload installed for that package. Production does not invoke `npx ...@latest`.

## Physical acceptance

Interactive UIA, native audio/clipboard, and full browser + desktop coordination are intentionally not asserted from a headless hosted runner. See `docs/physical-acceptance-v0.6.md` for the required physical Windows validation before release promotion.
