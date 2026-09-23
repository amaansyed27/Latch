# Latch V0.6 Setup

## Windows device

1. Install the V0.6 MSI on Windows 10/11 x64.
2. Open Latch Desktop and pair the PC from the Latch account/device page.
3. Approve only the folders ChatGPT should access through `latch_files`.
4. Review the fine-grained local capability policy. Start with `Observe` or `Work`; enable higher-authority capabilities only when needed.
5. Keep authenticated-browser access denied unless an explicit supported integration has been configured and physically validated.

The worker connects outbound to the Router over TLS WebSocket. No inbound public port is required.

## ChatGPT / MCP

Connect the MCP client to:

```text
https://latch-router.vercel.app/mcp
```

Complete OAuth in the browser. The Router grants only the approved OAuth scopes; the Windows PC still applies its local policy, Pause state, approved roots, device ownership, and Ask-mode approvals.

V0.6 exposes nine tools: `latch_devices`, `latch_session`, `latch_inspect`, `latch_files`, `latch_exec`, `latch_act`, `latch_browser`, `latch_tools`, and `latch_events`.

Typical task flow:

1. list/select the device;
2. create a Latch session;
3. open an approved workspace;
4. inspect semantic state before using screenshots;
5. use persistent terminal/browser identities across calls;
6. request deterministic verification for mutations that must be proved;
7. consume events after the saved cursor instead of repeatedly dumping full state;
8. close/clean task resources when finished.

## Local MCP integrations

Configure local MCP providers in Latch Desktop. For stdio integrations, store environment-variable references rather than copying secret values into remote configuration. Enable `allow_remote` only for providers that should be searchable/callable from ChatGPT.

ChatGPT uses `latch_tools` lazily: search returns a small candidate list, describe fetches only the selected schema, and call executes the opaque selected tool ref.

## Browser

The MSI includes the Latch browser runtime. `latch_browser` can create isolated/testing contexts and a Latch persistent profile. It does not claim control of an existing logged-in default Chrome profile.

## Troubleshooting

Use Latch Desktop → Diagnostics and local logs under `%LOCALAPPDATA%\Latch\logs`. Do not paste device credentials, browser profile data, or local MCP secret values into issue reports.
