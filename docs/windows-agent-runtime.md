# Windows Agent Runtime (V0.6)

Latch V0.6 moves the local runtime from a globally serialized command bridge to a session-oriented Windows agent runtime.

The Router remains a thin authenticated relay. Device-local managers own files, processes, persistent terminals, Windows UI Automation references, browser contexts/tabs, MCP connections, permissions, approvals, events, and verification state. Remote protocol messages use opaque IDs rather than operating-system handles or local secret-bearing configuration.

This document is intentionally implementation-facing. Physical interactive-Windows acceptance remains separate from hosted CI and is documented in the release checklist.
