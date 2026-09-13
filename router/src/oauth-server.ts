import { isIP } from 'node:net';
import type { IncomingHttpHeaders, IncomingMessage, ServerResponse } from 'node:http';

import type { AuthorizationStore, Principal } from './authorization-store.js';
import { LATCH_SCOPES, randomSecret, validScopes } from './authorization-store.js';
import { safeTokenEqual } from './auth.js';
import type { RouterConfig } from './config.js';
import { allowRequest } from './rate-limit.js';
import { isUuid } from './validation.js';

const MAX_BODY = 32 * 1024;

export function createOAuthHandler(config: RouterConfig, store: AuthorizationStore) {
  return (request: IncomingMessage, response: ServerResponse): void => {
    void handleOAuth(request, response, config, store).catch((error: unknown) => {
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

async function handleOAuth(request: IncomingMessage, response: ServerResponse, config: RouterConfig, store: AuthorizationStore): Promise<void> {
  const url = new URL(request.url ?? '/', config.publicBaseUrl);
  secureHeaders(response);
  const sensitive = url.pathname.startsWith('/oauth/') || url.pathname.startsWith('/api/pairing/');
  if (sensitive && !allowRequest(request, url.pathname, url.pathname === '/api/pairing/exchange' ? 20 : 60)) { json(response, 429, { error: 'rate_limited' }); return; }

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
    if (request.method === 'POST' && url.pathname === '/api/pairing/create') {
      json(response, 201, { pairing_code: await store.createPairingCode(userId, 10 * 60_000), expires_in: 600 });
      return;
    }
    if (request.method === 'GET') {
      json(response, 200, { devices: await store.listDevices(userId) });
      return;
    }
    if (request.method === 'DELETE') {
      const body = await jsonBody(request);
      const removed = body && typeof body.device_id === 'string' && await store.revokeDevice(userId, body.device_id);
      json(response, removed ? 200 : 404, removed ? { revoked: true } : { error: 'device_not_found' });
      return;
    }
  }
  if (url.pathname.startsWith('/api/auth/') && config.neonAuthBaseUrl) {
    if (request.method !== 'GET' && request.headers.origin !== config.publicBaseUrl) { json(response, 403, { error: 'csrf_rejected' }); return; }
    await proxyNeonAuth(request, response, url, config.neonAuthBaseUrl);
    return;
  }
  if (request.method === 'GET' && (url.pathname === '/' || url.pathname === '/login' || url.pathname === '/devices' || url.pathname === '/privacy' || url.pathname === '/terms' || url.pathname === '/support' || url.pathname === '/security')) {
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
    const hidden = [...url.searchParams].map(([name, value]) => `<input type="hidden" name="${html(name)}" value="${html(value)}">`).join('');
    response.statusCode = 200;
    response.setHeader('content-type', 'text/html; charset=utf-8');
    response.end(`<!doctype html><meta name="viewport" content="width=device-width"><title>Authorize Latch</title><style>body{max-width:40rem;margin:12vh auto;padding:1.5rem;font:18px system-ui;line-height:1.5}button{padding:.7rem 1rem;font:inherit}</style><h1>Authorize Latch</h1><p>Allow ChatGPT to use these capabilities on your paired computers:</p><ul>${scopes.map((scope) => `<li>${html(scope)}</li>`).join('')}</ul><p>Commands run with your OS account permissions and are not filesystem-sandboxed.</p><form method="post" action="/oauth/authorize">${hidden}<input type="hidden" name="csrf" value="${csrf}"><button name="approve" value="yes">Allow</button></form>`);
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
  const resource = body.get('resource') ?? '';
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
  response.setHeader('content-security-policy', "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'");
}
function json(response: ServerResponse, status: number, body: unknown): void { if (response.headersSent) return; response.statusCode = status; response.setHeader('content-type', 'application/json; charset=utf-8'); response.end(JSON.stringify(body)); }

function page(response: ServerResponse, path: string, authReady: boolean, returnTo: string | null): void {
  const pages: Record<string, [string, string]> = {
    '/': ['Latch', 'Use your own computer from ChatGPT. Pair a device, then let approved Latch tools work through the outbound encrypted connection.'],
    '/privacy': ['Privacy', 'Latch stores account, authorization, and device metadata. The relay does not durably store file contents, command output, source code, or conversation content.'],
    '/terms': ['Terms', 'You are responsible for commands you authorize. Latch commands run with the permissions of the OS user running latch-link.'],
    '/support': ['Support', 'Support: open an issue at github.com/amaansyed27/Latch. Never include credentials, pairing codes, or tokens.'],
    '/security': ['Security', 'Filesystem tools are workspace-confined. Command execution is not filesystem-sandboxed and can access anything available to the OS user running latch-link. Devices and OAuth grants can be revoked independently.'],
    '/devices': ['My devices', 'Create a short-lived pairing code, see your devices, and revoke access.'],
  };
  const [title, copy] = pages[path] ?? ['Sign in', authReady ? 'Sign in to authorize Latch and manage your paired computers.' : 'Managed sign-in is still provisioning.'];
  const login = path === '/login' && authReady ? `<form id="login"><input name="email" type="email" autocomplete="email" placeholder="Email" required><input name="password" type="password" autocomplete="current-password" placeholder="Password" required><button name="action" value="sign-in">Sign in</button><button name="action" value="sign-up">Create account</button></form><script>document.querySelector('#login').onsubmit=async(e)=>{e.preventDefault();const f=new FormData(e.target);const action=e.submitter.value;f.delete('action');const body=Object.fromEntries(f);if(action==='sign-up')body.name=body.email.split('@')[0];const r=await fetch('/api/auth/'+action+'/email',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});if(r.ok)location.href=${JSON.stringify(returnTo && returnTo.startsWith('/') ? returnTo : '/devices')};else alert('Authentication failed')}</script>` : '';
  const devices = path === '/devices' && authReady ? `<button id="pair">Create pairing code</button><pre id="code"></pre><ul id="devices"></ul><script>async function load(){const r=await fetch('/api/my/devices');if(r.status===401){location.href='/login?return_to=/devices';return}const j=await r.json(),list=document.querySelector('#devices');list.replaceChildren(...j.devices.map(d=>{const li=document.createElement('li'),code=document.createElement('code'),button=document.createElement('button');li.append(document.createTextNode(d.deviceName+' '));code.textContent=d.deviceId;button.textContent='Revoke';button.dataset.id=d.deviceId;li.append(code,' ',button);return li}))}document.querySelector('#pair').onclick=async()=>{const r=await fetch('/api/pairing/create',{method:'POST'});const j=await r.json();document.querySelector('#code').textContent=j.pairing_code?'Run: latch-link pair '+j.pairing_code:'Unable to create code'};document.querySelector('#devices').onclick=async(e)=>{if(e.target.dataset.id&&confirm('Revoke this device?')){await fetch('/api/my/devices',{method:'DELETE',headers:{'content-type':'application/json'},body:JSON.stringify({device_id:e.target.dataset.id})});load()}};load()</script>` : '';
  response.statusCode = 200; response.setHeader('content-type', 'text/html; charset=utf-8');
  response.end(`<!doctype html><meta name="viewport" content="width=device-width"><title>${title} · Latch</title><style>body{max-width:44rem;margin:12vh auto;padding:1.5rem;font:18px system-ui;line-height:1.55;color:#182018}nav a{margin-right:1rem}input,button{margin:.8rem .4rem .8rem 0;padding:.7rem;font:inherit;box-sizing:border-box}input{display:block;width:100%}code,pre{overflow-wrap:anywhere}</style><nav><a href="/">Latch</a><a href="/devices">Devices</a><a href="/security">Security</a><a href="/privacy">Privacy</a><a href="/terms">Terms</a><a href="/support">Support</a></nav><h1>${title}</h1><p>${copy}</p>${login}${devices}`);
}

function html(value: string): string { return value.replace(/[&<>"']/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]!); }
function oauthResource(config: RouterConfig): string { return `${config.publicBaseUrl}/mcp`; }
