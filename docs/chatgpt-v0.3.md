# Latch V0.3 integration note — superseded by V0.5

This document records the older V0.3 ChatGPT/Codex integration milestone. It is **not** current setup guidance.

For the current V0.5 architecture and setup, use:

- [README.md](../README.md)
- [docs/architecture.md](architecture.md)
- the live [Connect ChatGPT](https://latch-router.vercel.app/connect-chatgpt) page

## What changed after V0.3

V0.3 proved the first remote MCP path with a four-tool, bearer-token prototype. V0.5 replaced that model with the current personal-beta architecture:

```text
ChatGPT / Codex / MCP client
        | HTTPS Streamable HTTP + OAuth
        v
https://latch-router.vercel.app/mcp
        |
        v
Router -> Redis routing -> outbound TLS WebSocket -> latch-link -> latch-engine
```

V0.5 now provides:

- OAuth instead of the old single bearer-token ChatGPT proof
- device ownership checks before relay dispatch
- approved local roots and opaque `root_id` values
- normal workspace opening by `root_id` plus optional relative path, not arbitrary absolute remote paths
- first-class filesystem and managed-process tools
- explicit screenshot and Windows computer-control tools
- generic local MCP discovery and invocation
- local permission switches that can deny access even when OAuth granted the scope
- the Windows tray/desktop pairing experience

The Router remains a thin relay/control plane. Filesystem access, commands, computer use, and local MCP integrations execute on the paired computer.

## Current pairing flow

The primary V0.5 user flow is:

1. Install Latch for Windows.
2. Sign in at the Latch website and choose **Add computer**.
3. Generate a one-time pairing code.
4. Open **Latch Desktop** from Start or the system tray and paste the code.
5. Approve folders and local capabilities in Latch Desktop.
6. Connect ChatGPT or another OAuth-capable MCP client to `https://latch-router.vercel.app/mcp`.

The CLI remains available as a fallback/developer interface, but it is no longer the primary pairing experience.

## Current security boundary

Command execution remains intentionally **unsandboxed** and inherits the local user's operating-system permissions. Latch filesystem tools are confined to opened approved workspaces, but arbitrary child processes are not an OS sandbox.

Screen capture is explicit-request only. Local MCP configuration and secret environment-variable references remain local to the paired computer.

See [docs/architecture.md](architecture.md) for the current protocol-v2 and permission model.
