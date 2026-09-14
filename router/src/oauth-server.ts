import { isIP } from 'node:net';
import type { IncomingHttpHeaders, IncomingMessage, ServerResponse } from 'node:http';

import type { AuthorizationStore, Principal } from './authorization-store.js';
import { LATCH_SCOPES, randomSecret, validScopes } from './authorization-store.js';
import { safeTokenEqual } from './auth.js';
import type { RouterConfig } from './config.js';
import type { RelayCoordinator } from './coordinator.js';
import { allowRequest } from './rate-limit.js';
import { isUuid } from './validation.js';
import { escapeHtml, renderAuthorization, renderPage, serveAsset } from './web-ui.js';

const MAX_BODY = 32 * 1024;

export function createOAuthHandler(config: RouterConfig, store: AuthorizationStore, coordinator: RelayCoordinator) {
  return (request: IncomingMessage, response: ServerResponse): void => {
    void handleOAuth(request, response, config, store, coordinator).catch((error: unknown) => {
      console.error('authorization request failed', { error_name: error instanceof Error ? error.name : 'unknown' });
      json(response, 500, { error: 'server_error' });
    });
  };
}

export async function authenticateBearer(
  headers: IncomingHttpHeaders,
  config: RouterConfig,
  store: AuthorizationStore | undefined,
): Promise<Principal | 'legacy' | null> {
  const match = /^Bearer ([^\s]+)$/.exec(headers.authorization ?? '');
  if (!match) return null;
  if (config.allowLegacyAppToken && match[1] === config.appToken) return 'legacy';
  return store ? store.authenticateAccessToken(match[1]!, oauthResource(config)) : null;
}

async function handleOAuth(request: IncomingMessage, response: ServerResponse, config: RouterConfig, store: AuthorizationStore, coordinator: RelayCoordinator): Promise<void> {
  const url = new URL(request.url ?? '/', config.publicBaseUrl);
  secureHeaders(response);
  if (request.method === 'GET' && serveAsset(response, url.pathname)) return;
  const sensitive = url.pathname.startsWith('/oauth/') || url.pathname.startsWith('/api/auth/') || url.pathname.startsWith('/api/pairing/') || url.pathname === '/api/my/devices';
  if (sensitive && !(await allowRequest(coordinator, request, rateGroup(url.pathname), url.pathname === '/api/pairing/exchange' ? 20 : 60))) { json(response, 429, { error: 'rate_limited' }); return; }

  if (request.method === 'GET' && url.pathname === '/.well-known/oauth-protected-resource') {
    json(response, 200, { resource: oauthResource(config), authorization_servers: [config.publicBaseUrl], scopes_supported: LATCH_SCOPES, resource_documentation: `${config.publicBaseUrl}/security`, resource_policy_uri: `${config.publicBaseUrl}/privacy`, resource_tos_uri: `${config.publicBaseUrl}/terms` });
    return;
  }
  if (request.method === 'GET' && url.pathname === '/.well-known/oauth-authorization-server') {
    json(response, 200, {
      issuer: config.publicBaseUrl,
      authorization_response_iss_parameter_supported: false,
      authorization_endpoint: `${config.publicBaseUrl}/oauth/authorize`,
      token_endpoint: `${config.publicBaseUrl}/oauth/token`,
      revocation_endpoint: `${config.publicBaseUrl}/oauth/revoke`,
      registration_endpoint: `${config.publicBaseUrl}/oauth/register`,
      response_types_supported: ['code'],
      grant_types_supported: ['authorization_code', 'refresh_token'],
      code_challenge_methods_supported: ['S256'],
      scopes_supported: LATCH_SCOPES,
      token_endpoint_auth_methods_supported: ['none'],
      client_id_metadata_document_supported: true,
    });
    return;
  }
  if (request.method === 'POST' && url.pathname === '/oauth/register') {
    await registerClient(response, await jsonBody(request), store);
    return;
  }
  if ((request.method === 'GET' || request.method === 'POST') && url.pathname === '/oauth/authorize') {
    await authorize(request, response, url, config, store);
    return;
  }
  if (request.method === 'POST' && url.pathname === '/oauth/token') {
    await token(response, await form(request), config, store);
    return;
  }
  if (request.method === 'POST' && url.pathname === '/oauth/revoke') {
    const body = await form(request);
    if (body.get('token')) await store.revokeToken(body.get('token')!);
    response.statusCode = 200;
    response.end();
    return;
  }
  if (request.method === 'POST' && url.pathname === '/api/pairing/exchange') {
    const body = await jsonBody(request);
    const credential = body && typeof body.code === 'string' && typeof body.device_id === 'string' && isUuid(body.device_id) && typeof body.device_name === 'string' && body.device_name.trim().length > 0
      ? await store.exchangePairingCode(body.code, body.device_id, body.device_name.slice(0, 128)) : null;
    json(response, credential ? 200 : 400, credential ? { device_credential: credential } : { error: 'invalid_pairing_code' });
    return;
  }
  if (url.pathname === '/api/my/devices' || url.pathname === '/api/pairing/create') {
    if (request.method !== 'GET' && request.headers.origin !== config.publicBaseUrl) { json(response, 403, { error: 'csrf_rejected' }); return; }
    const identity = await neonSession(request.headers, config.neonAuthBaseUrl);
    if (!identity) { json(response, 401, { error: 'unauthorized' }); return; }
    const userId = await store.upsertUser(identity.id, identity.email);
    if (!(await allowRequest(coordinator, request, url.pathname, 60, 60_000, userId))) { json(response, 429, { error: 'rate_limited' }); return; }
    if (request.method === 'POST' && url.pathname === '/api/pairing/create') {
      json(response, 201, { pairing_code: await store.createPairingCode(userId, 10 * 60_000), expires_in: 600 });
      return;
    }
    if (request.method === 'GET') {
      const online = new Set((await coordinator.listDevices()).filter((device) => device.owner_user_id === userId).map((device) => device.device_id));
      json(response, 200, { devices: (await store.listDevices(userId)).map((device) => ({ ...device, online: online.has(device.deviceId) })) });
      return;
    }
    if (request.method === 'DELETE') {
      const body = await jsonBody(request);
      const removed = body && typeof body.device_id === 'string' && await store.revokeDevice(userId, body.device_id);
      if (removed) await coordinator.revokeDevice(body!.device_id as string);
      json(response, removed ? 200 : 404, removed ? { revoked: true } : { error: 'device_not_found' });
      return;
    }
  }
  if (url.pathname.startsWith('/api/auth/') && config.neonAuthBaseUrl) {
    if (request.method !== 'GET' && request.headers.origin !== config.publicBaseUrl) { json(response, 403, { error: 'csrf_rejected' }); return; }
    await proxyNeonAuth(request, response, url, config.neonAuthBaseUrl);
    return;
  }
  if (request.method === 'GET' && (url.pathname === '/' || url.pathname === '/login' || url.pathname === '/devices' || url.pathname === '/download' || url.pathname === '/privacy' || url.pathname === '/terms' || url.pathname === '/support' || url.pathname === '/security')) {
    page(response, url.pathname, config.neonAuthBaseUrl !== undefined, url.searchParams.get('return_to'));
    return;
  }
  json(response, 404, { error: 'not_found' });
}

async function authorize(request: IncomingMessage, response: ServerResponse, url: URL, config: RouterConfig, store: AuthorizationStore): Promise<void> {
  const params = request.method === 'POST' ? await form(request) : url.searchParams;
  const values = Object.fromEntries(params);
  if (values.response_type !== 'code' || values.code_challenge_method !== 'S256' || !values.code_challenge || !values.client_id || !values.redirect_uri || !values.resource || !values.scope || !values.state) {
    json(response, 400, { error: 'invalid_request' });
    return;
  }
  if (values.resource !== oauthResource(config)) {
    json(response, 400, { error: 'invalid_target' });
    return;
  }
  const scopes = validScopes(values.scope);
  const client = await loadClientMetadata(values.client_id, store);
  if (!scopes || !client?.redirect_uris.includes(values.redirect_uri)) {
    json(response, 400, { error: 'invalid_request' });
    return;
  }
  const identity = await neonSession(request.headers, config.neonAuthBaseUrl);
  if (!identity) {
    const returnTo = `${url.pathname}${url.search}`;
    response.statusCode = 302;
    response.setHeader('location', `/login?return_to=${encodeURIComponent(returnTo)}`);
    response.end();
    return;
  }
  if (request.method === 'GET') {
    const csrf = randomSecret(24);
    response.setHeader('set-cookie', `__Host-latch_csrf=${csrf}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=600`);
    const hidden = [...url.searchParams].map(([name, value]) => `<input type="hidden" name="${escapeHtml(name)}" value="${escapeHtml(value)}">`).join('');
    renderAuthorization(response, hidden, scopes, csrf);
    return;
  }
  const csrfCookie = /(?:^|;\s*)__Host-latch_csrf=([^;]+)/.exec(request.headers.cookie ?? '')?.[1];
  if (request.headers.origin !== config.publicBaseUrl || values.approve !== 'yes' || !values.csrf || !csrfCookie || !safeTokenEqual(values.csrf, csrfCookie)) {
    const redirect = new URL(values.redirect_uri);
    redirect.searchParams.set('error', 'access_denied');
    redirect.searchParams.set('state', values.state);
    response.statusCode = 302;
    response.setHeader('location', redirect.toString());
    response.end();
    return;
  }
  const userId = await store.upsertUser(identity.id, identity.email);
  const code = await store.createAuthorizationCode({ userId, clientId: values.client_id, redirectUri: values.redirect_uri, resource: values.resource, scopes, codeChallenge: values.code_challenge });
  const redirect = new URL(values.redirect_uri);
  redirect.searchParams.set('code', code);
  redirect.searchParams.set('state', values.state);
  response.statusCode = 302;
  response.setHeader('referrer-policy', 'no-referrer');
  response.setHeader('location', redirect.toString());
  response.end();
}

async function token(response: ServerResponse, body: URLSearchParams, config: RouterConfig, store: AuthorizationStore): Promise<void> {
  const grant = body.get('grant_type');
  const clientId = body.get('client_id') ?? '';
  const resource = body.get('resource') ?? (grant === 'refresh_token' ? oauthResource(config) : '');
  let result = null;
  if (resource !== oauthResource(config)) {
    json(response, 400, { error: 'invalid_target' });
    return;
  }
  if (grant === 'authorization_code') {
    const code = body.get('code');
    const verifier = body.get('code_verifier');
    const redirectUri = body.get('redirect_uri');
    if (code && verifier && redirectUri) result = await store.exchangeAuthorizationCode(code, verifier, { clientId, redirectUri, resource });
  } else if (grant === 'refresh_token') {
    const refresh = body.get('refresh_token');
    if (refresh) result = await store.refresh(refresh, clientId, resource);
  }
  if (!result) {
    json(response, 400, { error: 'invalid_grant' });
    return;
  }
  json(response, 200, { access_token: result.accessToken, token_type: 'Bearer', expires_in: result.expiresIn, refresh_token: result.refreshToken, scope: result.scopes.join(' ') });
}

async function registerClient(response: ServerResponse, body: Record<string, unknown> | null, store: AuthorizationStore): Promise<void> {
  const redirects = body?.redirect_uris;
  const tokenMethod = body?.token_endpoint_auth_method;
  const grants = body?.grant_types;
  const responses = body?.response_types;
  if (!Array.isArray(redirects) || redirects.length === 0 || redirects.length > 10 || !redirects.every((value): value is string => typeof value === 'string' && safeRedirect(value)) || (tokenMethod !== undefined && tokenMethod !== 'none') || (grants !== undefined && (!Array.isArray(grants) || grants.some((value) => value !== 'authorization_code' && value !== 'refresh_token'))) || (responses !== undefined && (!Array.isArray(responses) || responses.some((value) => value !== 'code')))) {
    json(response, 400, { error: 'invalid_client_metadata' });
    return;
  }
  const clientId = await store.registerClient(redirects);
  json(response, 201, { client_id: clientId, client_id_issued_at: Math.floor(Date.now() / 1000), redirect_uris: redirects, token_endpoint_auth_method: 'none', grant_types: ['authorization_code', 'refresh_token'], response_types: ['code'] });
}

async function loadClientMetadata(clientId: string, store: AuthorizationStore): Promise<{ redirect_uris: string[] } | null> {
  const registered = await store.getClient(clientId);
  if (registered) return { redirect_uris: registered };
  let url: URL;
  try { url = new URL(clientId); } catch { return null; }
  if (url.protocol !== 'https:' || isIP(url.hostname) !== 0 || url.username || url.password || url.hostname !== 'chatgpt.com') return null;
  const signal = AbortSignal.timeout(5_000);
  try {
    const response = await fetch(url, { signal, redirect: 'error', headers: { accept: 'application/json' } });
    if (!response.ok || Number(response.headers.get('content-length') ?? 0) > MAX_BODY) return null;
    const metadata = await response.json() as Record<string, unknown>;
    if (metadata.client_id !== clientId || !Array.isArray(metadata.redirect_uris)) return null;
    const redirect_uris = metadata.redirect_uris.filter((value): value is string => typeof value === 'string' && safeRedirect(value));
    return redirect_uris.length === metadata.redirect_uris.length ? { redirect_uris } : null;
  } catch { return null; }
}

function safeRedirect(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'https:' || (url.protocol === 'http:' && (url.hostname === '127.0.0.1' || url.hostname === 'localhost'));
  } catch { return false; }
}

async function neonSession(headers: IncomingHttpHeaders, baseUrl?: string): Promise<{ id: string; email?: string } | null> {
  if (!baseUrl) return null;
  try {
    const response = await fetch(`${baseUrl.replace(/\/$/, '')}/get-session`, { headers: { cookie: headers.cookie ?? '' }, redirect: 'error', signal: AbortSignal.timeout(5_000) });
    if (!response.ok) return null;
    const body = await response.json() as { user?: { id?: unknown; email?: unknown } };
    return typeof body.user?.id === 'string' ? { id: body.user.id, email: typeof body.user.email === 'string' ? body.user.email : undefined } : null;
  } catch { return null; }
}

async function proxyNeonAuth(request: IncomingMessage, response: ServerResponse, url: URL, baseUrl: string): Promise<void> {
  const body = request.method === 'GET' || request.method === 'HEAD' ? undefined : new Uint8Array(await rawBody(request));
  const upstream = await fetch(`${baseUrl.replace(/\/$/, '')}/${url.pathname.slice('/api/auth/'.length)}${url.search}`, {
    method: request.method,
    body,
    redirect: 'manual',
    headers: { 'content-type': request.headers['content-type'] ?? 'application/json', cookie: request.headers.cookie ?? '', origin: new URL(baseUrl).origin },
  });
  response.statusCode = upstream.status;
  for (const cookie of upstream.headers.getSetCookie()) response.appendHeader('set-cookie', cookie.replace(/;\s*Domain=[^;]+/i, ''));
  response.setHeader('content-type', upstream.headers.get('content-type') ?? 'application/json');
  response.end(Buffer.from(await upstream.arrayBuffer()));
}

async function form(request: IncomingMessage): Promise<URLSearchParams> { return new URLSearchParams((await rawBody(request)).toString('utf8')); }
async function rawBody(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = []; let size = 0;
  for await (const chunk of request) { const value = Buffer.from(chunk as Uint8Array); size += value.length; if (size > MAX_BODY) throw new Error('request_too_large'); chunks.push(value); }
  return Buffer.concat(chunks);
}
async function jsonBody(request: IncomingMessage): Promise<Record<string, unknown> | null> {
  try { const value = JSON.parse((await rawBody(request)).toString('utf8')) as unknown; return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null; } catch { return null; }
}

function secureHeaders(response: ServerResponse): void {
  response.setHeader('cache-control', 'no-store'); response.setHeader('referrer-policy', 'no-referrer');
  response.setHeader('x-content-type-options', 'nosniff'); response.setHeader('x-frame-options', 'DENY');
  response.setHeader('content-security-policy', "default-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'");
}
function json(response: ServerResponse, status: number, body: unknown): void { if (response.headersSent) return; response.statusCode = status; response.setHeader('content-type', 'application/json; charset=utf-8'); response.end(JSON.stringify(body)); }

function page(response: ServerResponse, path: string, authReady: boolean, returnTo: string | null): void { renderPage(response, path, authReady, returnTo); }
function oauthResource(config: RouterConfig): string { return `${config.publicBaseUrl}/mcp`; }
function rateGroup(path: string): string {
  if (path === '/oauth/authorize') return 'oauth-authorize';
  if (path === '/oauth/token') return 'oauth-token';
  if (path === '/oauth/register') return 'oauth-register';
  if (path === '/oauth/revoke') return 'oauth-revoke';
  if (path === '/api/pairing/create') return 'pairing-create';
  if (path === '/api/pairing/exchange') return 'pairing-exchange';
  if (path === '/api/my/devices') return 'device-management';
  if (path.startsWith('/api/auth/')) return 'managed-auth';
  return 'sensitive-other';
}
