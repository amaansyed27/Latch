import { createHash, timingSafeEqual } from 'node:crypto';
import type { IncomingHttpHeaders } from 'node:http';

export function safeTokenEqual(provided: string, expected: string): boolean {
  const providedHash = createHash('sha256').update(provided).digest();
  const expectedHash = createHash('sha256').update(expected).digest();
  return timingSafeEqual(providedHash, expectedHash);
}

export function bearerToken(headers: IncomingHttpHeaders): string | null {
  const authorization = headers.authorization;
  if (typeof authorization !== 'string') {
    return null;
  }
  const match = /^Bearer ([^\s]+)$/.exec(authorization);
  return match?.[1] ?? null;
}

export function isControlAuthorized(
  headers: IncomingHttpHeaders,
  expectedToken: string,
): boolean {
  const provided = bearerToken(headers);
  return provided !== null && safeTokenEqual(provided, expectedToken);
}

export function isAppAuthorized(
  headers: IncomingHttpHeaders,
  expectedToken: string,
): boolean {
  const provided = bearerToken(headers);
  return provided !== null && safeTokenEqual(provided, expectedToken);
}
