CREATE TABLE IF NOT EXISTS latch_users (
  id uuid PRIMARY KEY,
  provider_subject text NOT NULL UNIQUE,
  email text,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS latch_devices (
  device_id uuid PRIMARY KEY,
  owner_user_id uuid NOT NULL REFERENCES latch_users(id) ON DELETE CASCADE,
  device_name text NOT NULL,
  credential_hash text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  revoked_at timestamptz
);
CREATE INDEX IF NOT EXISTS latch_devices_owner_idx ON latch_devices(owner_user_id);

CREATE TABLE IF NOT EXISTS latch_pairing_codes (
  code_hash text PRIMARY KEY,
  owner_user_id uuid NOT NULL REFERENCES latch_users(id) ON DELETE CASCADE,
  expires_at timestamptz NOT NULL,
  used_at timestamptz
);

CREATE TABLE IF NOT EXISTS latch_oauth_codes (
  code_hash text PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES latch_users(id) ON DELETE CASCADE,
  client_id text NOT NULL,
  redirect_uri text NOT NULL,
  resource text NOT NULL,
  scopes text[] NOT NULL,
  code_challenge text NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz
);

CREATE TABLE IF NOT EXISTS latch_oauth_clients (
  client_id text PRIMARY KEY,
  redirect_uris text[] NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS latch_oauth_tokens (
  token_hash text PRIMARY KEY,
  family_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('access', 'refresh')),
  user_id uuid NOT NULL REFERENCES latch_users(id) ON DELETE CASCADE,
  client_id text NOT NULL,
  resource text NOT NULL,
  scopes text[] NOT NULL,
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz
);
CREATE INDEX IF NOT EXISTS latch_oauth_tokens_family_idx ON latch_oauth_tokens(family_id);

CREATE TABLE IF NOT EXISTS latch_oauth_families (
  family_id uuid PRIMARY KEY,
  revoked_at timestamptz
);
INSERT INTO latch_oauth_families (family_id)
SELECT DISTINCT family_id FROM latch_oauth_tokens ON CONFLICT DO NOTHING;
