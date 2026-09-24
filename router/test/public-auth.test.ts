import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import test from 'node:test';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { LATCH_SCOPES, MemoryAuthorizationStore, pkceChallenge } from '../src/authorization-store.js';
import { testConfig } from '../src/config.js';
import { MemoryCoordinator } from '../src/memory-coordinator.js';
import { createRouterRuntime } from '../src/runtime.js';
import { safeReturnTo } from '../src/web-ui.js';

const DEVICE_A = '00000000-0000-4000-8000-00000000000a';
const DEVICE_B = '00000000-0000-4000-8000-00000000000b';

void test('OAuth store enforces PKCE, one-time codes, refresh rotation, pairing, and revocation', async () => {
  let now = 1_000;
  const store = new MemoryAuthorizationStore(() => now);
  const user = await store.upsertUser('managed-user-a');
  const client = await store.registerClient(['http://127.0.0.1/callback']);
  assert.deepEqual(await store.getClient(client), ['http://127.0.0.1/callback']);
  const pairing = await store.createPairingCode(user, 100);
  const credential = await store.exchangePairingCode(pairing, DEVICE_A, 'Laptop');
  assert(credential);
  assert.equal(await store.exchangePairingCode(pairing, DEVICE_B, 'Other'), null);
  assert.equal((await store.authenticateDevice(DEVICE_A, credential))?.ownerUserId, user);
  assert.equal(await store.authenticateDevice(DEVICE_A, `${credential}x`), null);

  const verifier = 'v'.repeat(48);
  const request = { clientId: 'https://client.example/metadata.json', redirectUri: 'https://client.example/callback', resource: 'https://latch.example' };
  const code = await store.createAuthorizationCode({ ...request, userId: user, scopes: [...LATCH_SCOPES], codeChallenge: pkceChallenge(verifier) });
  assert.equal(await store.exchangeAuthorizationCode(code, 'wrong', request), null);
  const tokens = await store.exchangeAuthorizationCode(code, verifier, request);
  assert(tokens);
  assert.equal(await store.exchangeAuthorizationCode(code, verifier, request), null);
  assert.equal((await store.authenticateAccessToken(tokens.accessToken, request.resource))?.userId, user);
  assert.equal(await store.authenticateAccessToken(tokens.accessToken, 'https://wrong.example'), null);
  const rotated = await store.refresh(tokens.refreshToken, request.clientId, request.resource);
  assert(rotated);
  assert.equal(await store.refresh(tokens.refreshToken, request.clientId, request.resource), null);
  assert.equal(await store.refresh(rotated.refreshToken, request.clientId, request.resource), null);
  assert.equal(await store.authenticateAccessToken(rotated.accessToken, request.resource), null);

  assert(await store.revokeDevice(user, DEVICE_A));
  assert.equal(await store.authenticateDevice(DEVICE_A, credential), null);
  now += 1_000;
  const expired = await store.createPairingCode(user, 10);
  now += 11;
  assert.equal(await store.exchangePairingCode(expired, DEVICE_A, 'Laptop'), null);
});

void test('OAuth-authenticated MCP filters devices and scopes by user', async () => {
  const store = new MemoryAuthorizationStore();
  const userA = await store.upsertUser('user-a');
  const userB = await store.upsertUser('user-b');
  const coordinator = new MemoryCoordinator();
  const config = testConfig({ allowLegacyAppToken: false });
  const pairA = await store.createPairingCode(userA, 60_000);
  const pairB = await store.createPairingCode(userB, 60_000);
  assert(await store.exchangePairingCode(pairA, DEVICE_A, 'A laptop'));
  assert(await store.exchangePairingCode(pairB, DEVICE_B, 'B laptop'));
  const runtime = createRouterRuntime(config, coordinator, store);
  await runtime.ready;
  await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address(); assert(address && typeof address !== 'string');
  const baseUrl = `http://127.0.0.1:${address.port}`;
  config.publicBaseUrl = baseUrl;
  await coordinator.registerDevice({ device_id: DEVICE_A, device_name: 'A laptop', status: 'online', connected_at: new Date().toISOString(), instance_id: 'a', connection_id: 'a', owner_user_id: userA });
  await coordinator.registerDevice({ device_id: DEVICE_B, device_name: 'B laptop', status: 'online', connected_at: new Date().toISOString(), instance_id: 'b', connection_id: 'b', owner_user_id: userB });
  const verifier = 'p'.repeat(48);
  const request = { clientId: 'client', redirectUri: 'https://client/callback', resource: `${baseUrl}/mcp` };
  const code = await store.createAuthorizationCode({ ...request, userId: userA, scopes: ['latch:devices:read', 'latch:workspace:open', 'latch:exec:run'], codeChallenge: pkceChallenge(verifier) });
  const tokens = await store.exchangeAuthorizationCode(code, verifier, request); assert(tokens);
  const client = new Client({ name: 'oauth-test', version: '1' });
  const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), { requestInit: { headers: { authorization: `Bearer ${tokens.accessToken}` } } });
  try {
    await client.connect(transport);
    const tools = await client.listTools();
    assert.deepEqual(tools.tools[0]?._meta?.securitySchemes, [{ type: 'oauth2', scopes: ['latch:devices:read'] }]);
    const listed = await client.callTool({ name: 'latch_devices', arguments: {} });
    assert.deepEqual((listed.structuredContent as { devices: { device_id: string }[] }).devices.map((device) => device.device_id), [DEVICE_A]);
    const denied = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_B, request: { op: 'open_workspace', root_id: DEVICE_A } } });
    assert.equal(toolError(denied).code, 'device_offline');
    const underScoped = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_A, request: { op: 'read', workspace_id: DEVICE_B, path: 'README.md' } } });
    assert.equal(toolError(underScoped).code, 'insufficient_scope');
    assert(await store.revokeDevice(userA, DEVICE_A));
    await coordinator.revokeDevice(DEVICE_A);
    const afterRevoke = await client.callTool({ name: 'latch_exec', arguments: { device_id: DEVICE_A, request: { op: 'run', session_id: DEVICE_A, workspace_id: DEVICE_B, program: 'node', args: [] } } });
    assert.equal(toolError(afterRevoke).code, 'device_offline');
  } finally { await transport.close(); await runtime.close(); }
});

function toolError(result: unknown): { code: string; message: string } {
  const content = (result as { content: unknown }).content as { type: string; text: string }[];
  return (JSON.parse(content[0]!.text) as { error: { code: string; message: string } }).error;
}

void test('OAuth discovery is public and legacy app tokens are disabled by default', async () => {
  const config = testConfig({ allowLegacyAppToken: false });
  const store = new MemoryAuthorizationStore();
  const runtime = createRouterRuntime(config, new MemoryCoordinator(), store);
  await runtime.ready; await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address(); assert(address && typeof address !== 'string');
  const base = `http://127.0.0.1:${address.port}`;
  try {
    const metadata = await fetch(`${base}/.well-known/oauth-authorization-server`);
    assert.equal(metadata.status, 200);
    const metadataBody = await metadata.json() as { code_challenge_methods_supported: string[]; authorization_response_iss_parameter_supported: boolean };
    assert.deepEqual(metadataBody.code_challenge_methods_supported, ['S256']);
    assert.equal(metadataBody.authorization_response_iss_parameter_supported, false);
    const rewritten = await fetch(`${base}/api/router?latch_public_path=/.well-known/oauth-protected-resource`);
    assert.equal(rewritten.status, 200);
    assert.equal((await rewritten.json() as { resource: string }).resource, `${config.publicBaseUrl}/mcp`);
    const registration = await fetch(`${base}/oauth/register`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ redirect_uris: ['http://127.0.0.1/callback'], token_endpoint_auth_method: 'none' }) });
    assert.equal(registration.status, 201);
    assert.match((await registration.json() as { client_id: string }).client_id, /^latch_dcr_/);
    const unsafeRegistration = await fetch(`${base}/oauth/register`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ redirect_uris: ['javascript:alert(1)'] }) });
    assert.equal(unsafeRegistration.status, 400);
    const rejected = await fetch(`${base}/mcp`, { method: 'POST', headers: { authorization: `Bearer ${config.appToken}`, 'content-type': 'application/json' }, body: '{}' });
    assert.equal(rejected.status, 401);
    assert.match(rejected.headers.get('www-authenticate') ?? '', /oauth-protected-resource/);
    const user = await store.upsertUser('refresh-client');
    const verifier = 'r'.repeat(48);
    const tokenRequest = { clientId: 'refresh-client', redirectUri: 'https://client.example/callback', resource: `${config.publicBaseUrl}/mcp` };
    const code = await store.createAuthorizationCode({ ...tokenRequest, userId: user, scopes: ['latch:devices:read'], codeChallenge: pkceChallenge(verifier) });
    const issued = await store.exchangeAuthorizationCode(code, verifier, tokenRequest); assert(issued);
    const refreshResponse = await fetch(`${base}/oauth/token`, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ grant_type: 'refresh_token', client_id: tokenRequest.clientId, refresh_token: issued.refreshToken }) });
    assert.equal(refreshResponse.status, 200);
    for (const path of ['/', '/login', '/signup', '/forgot-password', '/reset-password', '/download', '/connect-chatgpt', '/security', '/privacy', '/terms', '/support', '/favicon.svg', '/theme.js']) {
      const page = await fetch(`${base}${path}`);
      assert.equal(page.status, 200, path);
      if (path === '/download') assert.match(await page.text(), /latch-bootstrap/);
    }
    const devices = await fetch(`${base}/devices`, { redirect: 'manual' });
    assert.equal(devices.status, 302);
    assert.equal(devices.headers.get('location'), '/login?return_to=%2Fdevices');
    assert.equal(safeReturnTo('//attacker.example'), '/dashboard');
    assert.equal(safeReturnTo('/devices'), '/devices');
  } finally { await runtime.close(); }
});

void test('distributed rate-limit buckets expire and revocation broadcasts immediately', async () => {
  const coordinator = new MemoryCoordinator();
  assert(await coordinator.allowRateLimit('oauth:hashed-subject', 2, 25));
  assert(await coordinator.allowRateLimit('oauth:hashed-subject', 2, 25));
  assert.equal(await coordinator.allowRateLimit('oauth:hashed-subject', 2, 25), false);
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert(await coordinator.allowRateLimit('oauth:hashed-subject', 2, 25));
  let revoked = '';
  await coordinator.subscribeRevocations(async (deviceId) => { revoked = deviceId; });
  await coordinator.revokeDevice(DEVICE_A);
  assert.equal(revoked, DEVICE_A);
});

void test('managed auth proxy preserves session cookies, protects pages, and signs out', async () => {
  const auth = createServer(async (request, response) => {
    if (request.url === '/get-session') {
      if (request.headers.cookie?.includes('session=valid')) response.end(JSON.stringify({ user: { id: 'managed-user', email: 'user@example.test' } }));
      else { response.statusCode = 401; response.end('{}'); }
      return;
    }
    if (request.url === '/sign-up/email' || request.url === '/sign-in/email') {
      response.setHeader('set-cookie', 'session=valid; Domain=auth.example; Path=/; HttpOnly; SameSite=Lax');
      response.end(JSON.stringify({ user: { id: 'managed-user' } })); return;
    }
    if (request.url === '/sign-out') {
      response.setHeader('set-cookie', 'session=; Domain=auth.example; Path=/; HttpOnly; Max-Age=0');
      response.end('{}'); return;
    }
    response.statusCode = 404; response.end('{}');
  });
  await new Promise<void>((resolve) => auth.listen(0, '127.0.0.1', resolve));
  const authAddress = auth.address(); assert(authAddress && typeof authAddress !== 'string');
  const config = testConfig({ neonAuthBaseUrl: `http://127.0.0.1:${authAddress.port}` });
  const runtime = createRouterRuntime(config, new MemoryCoordinator(), new MemoryAuthorizationStore());
  await runtime.ready; await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address(); assert(address && typeof address !== 'string');
  config.publicBaseUrl = `http://127.0.0.1:${address.port}`;
  try {
    const signedOut = await fetch(`${config.publicBaseUrl}/devices`, { redirect: 'manual' });
    assert.equal(signedOut.status, 302);
    const signup = await fetch(`${config.publicBaseUrl}/api/auth/sign-up/email`, { method: 'POST', headers: { origin: config.publicBaseUrl, 'content-type': 'application/json' }, body: JSON.stringify({ email: 'user@example.test', password: 'correct horse battery staple' }) });
    assert.equal(signup.status, 200);
    const cookie = signup.headers.get('set-cookie') ?? '';
    assert.match(cookie, /session=valid/);
    assert.doesNotMatch(cookie, /Domain=/i);
    const devices = await fetch(`${config.publicBaseUrl}/devices`, { headers: { cookie } });
    assert.equal(devices.status, 200);
    assert.match(await devices.text(), /user@example\.test/);
    const logout = await fetch(`${config.publicBaseUrl}/api/auth/sign-out`, { method: 'POST', headers: { origin: config.publicBaseUrl, cookie, 'content-type': 'application/json' }, body: '{}' });
    assert.equal(logout.status, 200);
    assert.match(logout.headers.get('set-cookie') ?? '', /Max-Age=0/);
  } finally {
    await runtime.close(); await new Promise<void>((resolve) => auth.close(() => resolve()));
  }
});
