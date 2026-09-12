export interface RouterConfig {
  pairingToken: string;
  controlToken: string;
  requestTimeoutMs: number;
  presenceTtlSeconds: number;
  heartbeatMs: number;
}

export interface ProductionConfig extends RouterConfig {
  redisUrl: string;
}

export type ConfigResult =
  | { ok: true; config: ProductionConfig }
  | { ok: false; missing: string[] };

const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;

export function loadProductionConfig(
  environment: NodeJS.ProcessEnv,
): ConfigResult {
  const required = [
    'LATCH_PAIRING_TOKEN',
    'LATCH_CONTROL_TOKEN',
    'LATCH_REDIS_URL',
  ] as const;
  const missing = required.filter((name) => !environment[name]?.trim());
  if (missing.length > 0) {
    return { ok: false, missing: [...missing] };
  }

  return {
    ok: true,
    config: {
      pairingToken: environment.LATCH_PAIRING_TOKEN as string,
      controlToken: environment.LATCH_CONTROL_TOKEN as string,
      redisUrl: environment.LATCH_REDIS_URL as string,
      requestTimeoutMs: parseTimeout(environment.LATCH_REQUEST_TIMEOUT_MS),
      presenceTtlSeconds: 90,
      heartbeatMs: 20_000,
    },
  };
}

export function testConfig(overrides: Partial<RouterConfig> = {}): RouterConfig {
  return {
    pairingToken: 'pairing-test-token',
    controlToken: 'control-test-token',
    requestTimeoutMs: 500,
    presenceTtlSeconds: 90,
    heartbeatMs: 20_000,
    ...overrides,
  };
}

function parseTimeout(raw: string | undefined): number {
  if (raw === undefined) {
    return DEFAULT_REQUEST_TIMEOUT_MS;
  }
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value) || value < 1_000 || value > 120_000) {
    return DEFAULT_REQUEST_TIMEOUT_MS;
  }
  return value;
}
