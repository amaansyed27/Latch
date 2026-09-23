# v0.6.0-beta.1 Release Notes

V0.6 is the Windows Agent Runtime private beta.

## Added

- explicit cross-surface sessions and degraded restart semantics
- concurrent engine dispatch without one global expensive-operation mutex
- semantic Windows UI Automation with opaque session-scoped refs
- native app, clipboard, and audio providers
- persistent ConPTY-backed terminals with bounded cursor output
- persistent pinned Playwright browser runtime with semantic snapshots, console/network/download cursors, screenshots, and persistent tabs
- deterministic Observe → Act → Verify results
- persistent local MCP federation and lazy search → describe → call
- bounded/coalesced event bus with long-wait
- fine-grained local deny/ask/allow permission policy and local approvals
- nine-tool public MCP surface
- bundled Node + Playwright `1.63.0` + Chromium in the Windows package
- deterministic developer-loop fixture and physical Windows acceptance harness

## Security behavior

- approved-root filesystem confinement is retained
- terminal/command execution is explicitly not sandboxed
- no UIPI bypass, silent elevation, secure-desktop automation, or SYSTEM service
- denied capabilities do not fall through to weaker routes to evade policy
- authenticated existing-browser control remains separately permissioned and not claimed complete
- Router remains a thin relay and does not intentionally persist local content/log/screenshot/browser/MCP-secret payloads

## Compatibility

V0.5 internal request variants remain available as a temporary compatibility path, while normal ChatGPT uses protocol v3 through the consolidated V0.6 public MCP tools.

## Validation boundary

CI validates static quality, Rust workspace tests/build, Router tests/build/web tests, Router→local E2E, MCP scale/lazy discovery, deterministic fixture behavior, Windows build/package payload, MSI ProductVersion `0.6.0`, clean install/uninstall, and release checksums.

Interactive UIA, native audio/clipboard, and complete cross-surface physical workflows still require the exact-release-candidate Windows checklist in `docs/physical-acceptance-v0.6.md` before physical validation can be claimed.
