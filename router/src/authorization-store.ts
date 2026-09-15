import { createHash, randomBytes, randomUUID } from 'node:crypto';

import { neon, type NeonQueryFunction } from '@neondatabase/serverless';

export const LATCH_SCOPES = [
  'latch:devices:read',
  'latch:roots:read',
  'latch:workspace:open',
  'latch:files:read',
  'latch:files:write',
  'latch:exec:run',
  'latch:computer:read',
  'latch:computer:control',
  'latch:mcp:read',
  'latch:mcp:call',
] as const;
export type LatchScope = (typeof LATCH_SCOPES)[number];

export interface Principal {
  userId: string;
  clientId: string;
  scopes: string[];
  resource: string;
}

export interface OwnedDevice {
  deviceId: string;
  ownerUserId: string;
  deviceName: string;
  pairedAt?: string;
}

export interface AuthorizationStore {
  upsertUser(subject: string, email?: string): Promise<string>;
  createPairingCode(userId: string, ttlMs: number): Promise<string>;
  exchangePairingCode(code: string, deviceId: string, deviceName: string): Promise<string | null>;
  authenticateDevice(deviceId: string, credential: string): Promise<OwnedDevice | null>;
  listDevices(userId: string): Promise<OwnedDevice[]>;
  ownsDevice(userId: string, deviceId: string): Promise<boolean>;
  revokeDevice(userId: string, deviceId: string): Promise<boolean>;
  registerClient(redirectUris: string[]): Promise<string>;
  getClient(clientId: string): Promise<string[] | null>;
  createAuthorizationCode(input: AuthorizationCodeInput): Promise<string>;
  exchangeAuthorizationCode(code: string, verifier: string, input: TokenRequest): Promise<TokenSet | null>;
  refresh(refreshToken: string, clientId: string, resource: string): Promise<TokenSet | null>;
  authenticateAccessToken(token: string, resource: string): Promise<Principal | null>;
  revokeToken(token: string): Promise<void>;
}

export interface AuthorizationCodeInput extends TokenRequest {
  userId: string;
  scopes: string[];
  codeChallenge: string;
}

export interface TokenRequest {
  clientId: string;
  redirectUri: string;
  resource: string;
}

export interface TokenSet {
  accessToken: string;
  refreshToken: string;
  expiresIn: number;
  scopes: string[];
}

const ACCESS_TTL_SECONDS = 15 * 60;
const REFRESH_TTL_SECONDS = 30 * 24 * 60 * 60;

export class PostgresAuthorizationStore implements AuthorizationStore {
  readonly #sql: NeonQueryFunction<false, false>;

  constructor(databaseUrl: string) {
    this.#sql = neon(databaseUrl);
  }

  async upsertUser(subject: string, email?: string): Promise<string> {
    const id = randomUUID();
    const rows = await this.#sql`
      INSERT INTO latch_users (id, provider_subject, email)
      VALUES (${id}, ${subject}, ${email ?? null})
      ON CONFLICT (provider_subject) DO UPDATE SET email = COALESCE(EXCLUDED.email, latch_users.email)
      RETURNING id::text`;
    return String(rows[0]?.id);
  }

  async createPairingCode(userId: string, ttlMs: number): Promise<string> {
    const code = randomSecret(24);
    await this.#sql`INSERT INTO latch_pairing_codes (code_hash, owner_user_id, expires_at)
      VALUES (${hash(code)}, ${userId}, now() + (${ttlMs} * interval '1 millisecond'))`;
    return code;
  }

  async exchangePairingCode(code: string, deviceId: string, deviceName: string): Promise<string | null> {
    const credential = randomSecret(32);
    const rows = await this.#sql`
      WITH claimed AS (
        UPDATE latch_pairing_codes SET used_at = now()
        WHERE code_hash = ${hash(code)} AND used_at IS NULL AND expires_at > now()
        RETURNING owner_user_id
      )
      INSERT INTO latch_devices (device_id, owner_user_id, device_name, credential_hash, revoked_at)
      SELECT ${deviceId}, owner_user_id, ${deviceName}, ${hash(credential)}, NULL FROM claimed
      ON CONFLICT (device_id) DO UPDATE SET
        device_name = EXCLUDED.device_name,
        credential_hash = EXCLUDED.credential_hash,
        revoked_at = NULL
      WHERE latch_devices.owner_user_id = EXCLUDED.owner_user_id
      RETURNING device_id`;
    return rows.length === 1 ? credential : null;
  }

  async authenticateDevice(deviceId: string, credential: string): Promise<OwnedDevice | null> {
    const rows = await this.#sql`SELECT device_id::text, owner_user_id::text, device_name, created_at
      FROM latch_devices WHERE device_id = ${deviceId} AND credential_hash = ${hash(credential)} AND revoked_at IS NULL`;
    return deviceRow(rows[0]);
  }

  async listDevices(userId: string): Promise<OwnedDevice[]> {
    const rows = await this.#sql`SELECT device_id::text, owner_user_id::text, device_name, created_at
      FROM latch_devices WHERE owner_user_id = ${userId} AND revoked_at IS NULL ORDER BY device_name`;
    return rows.map(deviceRow).filter((row): row is OwnedDevice => row !== null);
  }

  async ownsDevice(userId: string, deviceId: string): Promise<boolean> {
    const rows = await this.#sql`SELECT 1 FROM latch_devices
      WHERE owner_user_id = ${userId} AND device_id = ${deviceId} AND revoked_at IS NULL`;
    return rows.length === 1;
  }

  async revokeDevice(userId: string, deviceId: string): Promise<boolean> {
    const rows = await this.#sql`UPDATE latch_devices SET revoked_at = now()
      WHERE owner_user_id = ${userId} AND device_id = ${deviceId} AND revoked_at IS NULL RETURNING device_id`;
    return rows.length === 1;
  }

  async registerClient(redirectUris: string[]): Promise<string> {
    const clientId = `latch_dcr_${randomSecret(24)}`;
    await this.#sql`INSERT INTO latch_oauth_clients (client_id, redirect_uris) VALUES (${clientId}, ${redirectUris})`;
    return clientId;
  }

  async getClient(clientId: string): Promise<string[] | null> {
    const rows = await this.#sql`SELECT redirect_uris FROM latch_oauth_clients WHERE client_id = ${clientId}`;
    return rows[0] ? rows[0].redirect_uris as string[] : null;
  }

  async createAuthorizationCode(input: AuthorizationCodeInput): Promise<string> {
    const code = randomSecret(32);
    await this.#sql`INSERT INTO latch_oauth_codes
      (code_hash, user_id, client_id, redirect_uri, resource, scopes, code_challenge, expires_at)
      VALUES (${hash(code)}, ${input.userId}, ${input.clientId}, ${input.redirectUri}, ${input.resource},
        ${input.scopes}, ${input.codeChallenge}, now() + interval '5 minutes')`;
    return code;
  }

  async exchangeAuthorizationCode(code: string, verifier: string, input: TokenRequest): Promise<TokenSet | null> {
    const rows = await this.#sql`UPDATE latch_oauth_codes SET used_at = now()
      WHERE code_hash = ${hash(code)} AND used_at IS NULL AND expires_at > now()
        AND client_id = ${input.clientId} AND redirect_uri = ${input.redirectUri} AND resource = ${input.resource}
        AND code_challenge = ${pkceChallenge(verifier)}
      RETURNING user_id::text, scopes`;
    const row = rows[0];
    if (!row) return null;
    return this.#issueTokens(String(row.user_id), input.clientId, input.resource, row.scopes as string[]);
  }

  async refresh(refreshToken: string, clientId: string, resource: string): Promise<TokenSet | null> {
    const tokenHash = hash(refreshToken);
    const replayed = await this.#sql`UPDATE latch_oauth_families SET revoked_at = now()
      WHERE family_id = (SELECT family_id FROM latch_oauth_tokens
        WHERE token_hash = ${tokenHash} AND kind = 'refresh' AND revoked_at IS NOT NULL)
      RETURNING family_id`;
    if (replayed.length > 0) return null;
    const rows = await this.#sql`UPDATE latch_oauth_tokens SET revoked_at = now()
      WHERE token_hash = ${tokenHash} AND kind = 'refresh' AND client_id = ${clientId}
        AND resource = ${resource} AND revoked_at IS NULL AND expires_at > now()
        AND EXISTS (SELECT 1 FROM latch_oauth_families f WHERE f.family_id = latch_oauth_tokens.family_id AND f.revoked_at IS NULL)
      RETURNING user_id::text, scopes, family_id::text`;
    const row = rows[0];
    if (!row) return null;
    return this.#issueTokens(String(row.user_id), clientId, resource, row.scopes as string[], String(row.family_id));
  }

  async authenticateAccessToken(token: string, resource: string): Promise<Principal | null> {
    const rows = await this.#sql`SELECT user_id::text, client_id, scopes, resource
      FROM latch_oauth_tokens t WHERE token_hash = ${hash(token)} AND kind = 'access'
        AND resource = ${resource} AND revoked_at IS NULL AND expires_at > now()
        AND EXISTS (SELECT 1 FROM latch_oauth_families f WHERE f.family_id = t.family_id AND f.revoked_at IS NULL)`;
    const row = rows[0];
    return row ? { userId: String(row.user_id), clientId: String(row.client_id), scopes: row.scopes as string[], resource: String(row.resource) } : null;
  }

  async revokeToken(token: string): Promise<void> {
    await this.#sql`UPDATE latch_oauth_families SET revoked_at = now()
      WHERE family_id = (SELECT family_id FROM latch_oauth_tokens WHERE token_hash = ${hash(token)})`;
  }

  async #issueTokens(userId: string, clientId: string, resource: string, scopes: string[], familyId: string = randomUUID()): Promise<TokenSet> {
    const accessToken = randomSecret(32);
    const refreshToken = randomSecret(48);
    await this.#sql.transaction([
      this.#sql`INSERT INTO latch_oauth_families (family_id) VALUES (${familyId}) ON CONFLICT DO NOTHING`,
      this.#sql`INSERT INTO latch_oauth_tokens
        (token_hash, family_id, kind, user_id, client_id, resource, scopes, expires_at)
        VALUES (${hash(accessToken)}, ${familyId}, 'access', ${userId}, ${clientId}, ${resource}, ${scopes}, now() + interval '15 minutes')`,
      this.#sql`INSERT INTO latch_oauth_tokens
        (token_hash, family_id, kind, user_id, client_id, resource, scopes, expires_at)
        VALUES (${hash(refreshToken)}, ${familyId}, 'refresh', ${userId}, ${clientId}, ${resource}, ${scopes}, now() + interval '30 days')`,
    ]);
    return { accessToken, refreshToken, expiresIn: ACCESS_TTL_SECONDS, scopes };
  }
}

interface MemoryCode extends AuthorizationCodeInput { expiresAt: number; used: boolean }
interface MemoryToken extends Principal { familyId: string; kind: 'access' | 'refresh'; expiresAt: number; revoked: boolean }

export class MemoryAuthorizationStore implements AuthorizationStore {
  readonly #users = new Map<string, string>();
  readonly #devices = new Map<string, OwnedDevice & { credentialHash: string; revoked: boolean }>();
  readonly #pairing = new Map<string, { userId: string; expiresAt: number; used: boolean }>();
  readonly #clients = new Map<string, string[]>();
  readonly #codes = new Map<string, MemoryCode>();
  readonly #tokens = new Map<string, MemoryToken>();
  constructor(private readonly now: () => number = Date.now) {}

  async upsertUser(subject: string): Promise<string> {
    const current = this.#users.get(subject); if (current) return current;
    const id = randomUUID(); this.#users.set(subject, id); return id;
  }
  async createPairingCode(userId: string, ttlMs: number): Promise<string> {
    const code = randomSecret(24); this.#pairing.set(hash(code), { userId, expiresAt: this.now() + ttlMs, used: false }); return code;
  }
  async exchangePairingCode(code: string, deviceId: string, deviceName: string): Promise<string | null> {
    const record = this.#pairing.get(hash(code));
    if (!record || record.used || record.expiresAt <= this.now()) return null;
    record.used = true;
    const existing = this.#devices.get(deviceId);
    if (existing && existing.ownerUserId !== record.userId) return null;
    const credential = randomSecret(32);
    this.#devices.set(deviceId, { deviceId, ownerUserId: record.userId, deviceName, pairedAt: existing?.pairedAt ?? new Date(this.now()).toISOString(), credentialHash: hash(credential), revoked: false });
    return credential;
  }
  async authenticateDevice(deviceId: string, credential: string): Promise<OwnedDevice | null> {
    const record = this.#devices.get(deviceId);
    return record && !record.revoked && record.credentialHash === hash(credential) ? record : null;
  }
  async listDevices(userId: string): Promise<OwnedDevice[]> { return [...this.#devices.values()].filter((device) => device.ownerUserId === userId && !device.revoked); }
  async ownsDevice(userId: string, deviceId: string): Promise<boolean> { const device = this.#devices.get(deviceId); return device?.ownerUserId === userId && !device.revoked; }
  async revokeDevice(userId: string, deviceId: string): Promise<boolean> { const device = this.#devices.get(deviceId); if (!device || device.ownerUserId !== userId || device.revoked) return false; device.revoked = true; return true; }
  async registerClient(redirectUris: string[]): Promise<string> { const id = `latch_dcr_${randomSecret(24)}`; this.#clients.set(id, redirectUris); return id; }
  async getClient(clientId: string): Promise<string[] | null> { return this.#clients.get(clientId) ?? null; }
  async createAuthorizationCode(input: AuthorizationCodeInput): Promise<string> { const code = randomSecret(32); this.#codes.set(hash(code), { ...input, expiresAt: this.now() + 300_000, used: false }); return code; }
  async exchangeAuthorizationCode(code: string, verifier: string, input: TokenRequest): Promise<TokenSet | null> {
    const record = this.#codes.get(hash(code));
    if (!record || record.used || record.expiresAt <= this.now() || record.clientId !== input.clientId || record.redirectUri !== input.redirectUri || record.resource !== input.resource || record.codeChallenge !== pkceChallenge(verifier)) return null;
    record.used = true; return this.#issue(record.userId, record.clientId, record.resource, record.scopes);
  }
  async refresh(refreshToken: string, clientId: string, resource: string): Promise<TokenSet | null> {
    const record = this.#tokens.get(hash(refreshToken));
    if (!record || record.kind !== 'refresh' || record.expiresAt <= this.now() || record.clientId !== clientId || record.resource !== resource) return null;
    if (record.revoked) {
      for (const value of this.#tokens.values()) if (value.familyId === record.familyId) value.revoked = true;
      return null;
    }
    record.revoked = true; return this.#issue(record.userId, clientId, resource, record.scopes, record.familyId);
  }
  async authenticateAccessToken(token: string, resource: string): Promise<Principal | null> { const record = this.#tokens.get(hash(token)); return record && record.kind === 'access' && !record.revoked && record.expiresAt > this.now() && record.resource === resource ? record : null; }
  async revokeToken(token: string): Promise<void> { const record = this.#tokens.get(hash(token)); if (record) for (const value of this.#tokens.values()) if (value.familyId === record.familyId) value.revoked = true; }
  #issue(userId: string, clientId: string, resource: string, scopes: string[], familyId: string = randomUUID()): TokenSet {
    const accessToken = randomSecret(32); const refreshToken = randomSecret(48);
    this.#tokens.set(hash(accessToken), { userId, clientId, resource, scopes, familyId, kind: 'access', expiresAt: this.now() + ACCESS_TTL_SECONDS * 1000, revoked: false });
    this.#tokens.set(hash(refreshToken), { userId, clientId, resource, scopes, familyId, kind: 'refresh', expiresAt: this.now() + REFRESH_TTL_SECONDS * 1000, revoked: false });
    return { accessToken, refreshToken, expiresIn: ACCESS_TTL_SECONDS, scopes };
  }
}

function deviceRow(row: Record<string, unknown> | undefined): OwnedDevice | null {
  return row ? { deviceId: String(row.device_id), ownerUserId: String(row.owner_user_id), deviceName: String(row.device_name), ...(row.created_at ? { pairedAt: new Date(String(row.created_at)).toISOString() } : {}) } : null;
}

export function hash(value: string): string {
  return createHash('sha256').update(value).digest('base64url');
}

export function pkceChallenge(verifier: string): string {
  return hash(verifier);
}

export function randomSecret(bytes: number): string {
  return randomBytes(bytes).toString('base64url');
}

export function validScopes(raw: string): string[] | null {
  const scopes = [...new Set(raw.split(/\s+/).filter(Boolean))];
  return scopes.length > 0 && scopes.every((scope) => (LATCH_SCOPES as readonly string[]).includes(scope)) ? scopes : null;
}

export { ACCESS_TTL_SECONDS, REFRESH_TTL_SECONDS };
