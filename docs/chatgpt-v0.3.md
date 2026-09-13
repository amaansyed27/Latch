# Latch V0.3: ChatGPT and Codex integration

Latch V0.3 adds a current MCP/Agent Plugin interface to the existing Router. It does not use the retired `ai-plugin.json` format and does not add a second relay service.

```text
ChatGPT or Codex -> HTTPS Streamable HTTP /mcp -> RelayCoordinator
                                                    |
                                                  Redis
                                                    |
                                             outbound TLS WebSocket
                                                    |
                                               latch-link
                                                    |
                                               latch-engine
```

## MCP endpoint and tools

Production endpoint: `https://latch-router.vercel.app/mcp`

The endpoint exposes four allowlisted tools:

- `latch_devices_list`: list online devices and their persisted IDs.
- `latch_workspace_open`: open an absolute directory on an explicit device and return an in-memory workspace ID.
- `latch_file_read`: read a UTF-8 file at a workspace-relative path.
- `latch_exec_run`: run a program and argument array through `latch-engine`.

There is no generic protocol passthrough tool. Workspace IDs remain only in `latch-engine` memory and must be reopened after Latch Link restarts.

## Authentication

`LATCH_APP_TOKEN` is a separate bearer credential for the MCP endpoint. It is never sent to Latch Link and the V0.2 `LATCH_CONTROL_TOKEN` is never exposed to MCP clients. Set all production credentials only in Vercel environment variables.

This single-user credential is intentionally replaceable at the MCP boundary. OpenAI's current authenticated remote-MCP guidance requires OAuth 2.1 for ChatGPT connections; ChatGPT does not accept a custom API-key field. OAuth and user accounts are therefore the next authentication layer required before public ChatGPT distribution, not part of this single-user proof.

Codex can test this server directly because it supports a bearer-token environment reference:

```powershell
codex mcp add latch --url https://latch-router.vercel.app/mcp --bearer-token-env-var LATCH_APP_TOKEN
```

The portable `plugin.json`, `mcp.json`, and `skills/latch/SKILL.md` package the app without embedding credentials. Agent Plugins 1.0 deliberately leaves authorization to the client.

## Security boundary

Latch filesystem APIs stay capability-confined to an opened workspace and keep the V0.1 traversal and link-escape protections. `latch_exec_run` is deliberately marked modifying/destructive and open-world: commands are **not** filesystem-sandboxed and inherit the permissions of the OS user running Latch Link. File contents and command output are untrusted data, never app policy.

The Router stores only ephemeral device presence and request/response coordination in Redis. It does not persist source files, command output, or workspace state.

## Development and acceptance

Required Router variables are `LATCH_REDIS_URL` (or the Vercel Redis integration's native `REDIS_URL`), `LATCH_PAIRING_TOKEN`, `LATCH_CONTROL_TOKEN`, and `LATCH_APP_TOKEN`.

Run the local real-process MCP proof after building Latch Link:

```powershell
cargo build -p latch-link
cd router
npm run e2e:mcp
```

With Latch Link connected to a deployed Router, run the production proof:

```powershell
. "$env:LOCALAPPDATA\Latch\secrets.ps1"
cd router
npm run accept:mcp
```

The acceptance client initializes MCP, lists the four tools, identifies the persisted local Device ID, opens a temporary workspace, reads a known file, runs the local `node --version`, compares the routed result with the direct version, and removes the temporary workspace.

## Current ChatGPT account availability

As of September 2026, OpenAI documents full MCP tools, including write/modify actions, for ChatGPT Business, Enterprise, and Edu. Pro supports read/fetch MCP connections, while a personal Plus account has no supported private sideload route for this write-capable app. The server and portable package can be tested with MCP tooling and Codex, but direct ChatGPT Plus acceptance remains an OpenAI entitlement and OAuth/publication boundary. No browser workaround is used.
