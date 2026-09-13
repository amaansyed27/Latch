# Latch V0.4: public authentication and device ownership

Latch V0.4 keeps the existing relay and adds an OAuth 2.1 authorization boundary backed by Neon Postgres and managed Neon Auth identity.

```text
ChatGPT / Codex -- OAuth access token --> /mcp -- owner check --> Redis relay
                                                                    |
                                                          outbound TLS latch-link
                                                                    |
                                                               latch-engine
```

Official references: [OpenAI plugin authentication](https://developers.openai.com/plugins/build/auth) and [plugin submission](https://developers.openai.com/plugins/deploy/submission).

## OAuth

- Protected resource metadata: `/.well-known/oauth-protected-resource`
- Authorization server metadata: `/.well-known/oauth-authorization-server`
- Authorization Code with PKCE S256 and Client ID Metadata Documents (CIMD)
- opaque access tokens: 15 minutes; opaque refresh tokens: 30 days with rotation
- exact `resource` and redirect validation, single-use five-minute codes, hashed token persistence, grant revocation
- scopes: `latch:devices:read`, `latch:workspace:open`, `latch:files:read`, `latch:exec:run`

Production accepts ChatGPT-hosted CIMD documents. `LATCH_APP_TOKEN` is a test compatibility path only when `LATCH_ALLOW_LEGACY_APP_TOKEN=true`; production leaves it disabled.

## Device enrollment

Sign in at `/login`, open `/devices`, and create a ten-minute one-time pairing code. Then run:

```powershell
$env:LATCH_ROUTER_URL="https://latch-router.vercel.app"
$env:LATCH_DEVICE_NAME=$env:COMPUTERNAME
cargo run -p latch-link -- pair <code>
cargo run -p latch-link
```

The server stores only hashes of pairing and device credentials. On Windows, latch-link stores its unique credential in Windows Credential Manager. The persisted DeviceId remains in `%LOCALAPPDATA%\Latch\device.json`. Revoking a device blocks requests and reconnects; an active connection is closed at its next heartbeat.

## Durable and ephemeral data

`router/migrations/0001_public_auth.sql` defines users, owned devices, hashed pairing codes, authorization codes, and hashed OAuth tokens. Apply it with `npm run db:migrate` using `DATABASE_URL_UNPOOLED` (preferred) or `DATABASE_URL`. Redis remains limited to presence and request/response correlation. No file contents, source, command output, or conversations are durably stored.

## Security boundary

Filesystem tools remain workspace-confined. Commands are not filesystem-sandboxed: `latch_exec_run` inherits every permission of the OS user running latch-link. File and command output is untrusted data. Every MCP operation filters devices by the authenticated owner and deliberately returns not-found behavior for cross-user IDs.

## Submission material

- Name: Latch
- Short description: Use your own computer from ChatGPT.
- Category: Developer tools
- Universal MCP URL: `https://latch-router.vercel.app/mcp`
- Website: `https://latch-router.vercel.app/`
- Privacy: `https://latch-router.vercel.app/privacy`
- Terms: `https://latch-router.vercel.app/terms`
- Support: `https://latch-router.vercel.app/support`

Starter prompts:

1. List my connected computers.
2. Run node --version on my laptop.
3. Open my project and read Cargo.toml.
4. Run the tests in my project.

Positive review cases: list only the signed-in user's devices; open a valid workspace; read a workspace-relative file; run a harmless command with approval; reopen an expired workspace and retry. Negative cases: reject another user's DeviceId; reject traversal outside a workspace; reject an expired, reused, revoked, or under-scoped grant.

The OpenAI submission portal additionally requires Apps Management: Write, a verified developer or business identity, country availability, final policy attestations, a logo, demo credentials, and any domain-verification challenge. Those identity/legal choices must be completed by the publisher, not fabricated in source.
