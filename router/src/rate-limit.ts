import { createHash } from 'node:crypto';
import type { IncomingMessage } from 'node:http';

import type { RelayCoordinator } from './coordinator.js';

export async function allowRequest(coordinator: RelayCoordinator, request: IncomingMessage, group: string, limit: number, windowMs = 60_000, subject?: string): Promise<boolean> {
  const ip = String(request.headers['x-forwarded-for'] ?? request.socket.remoteAddress ?? 'unknown').split(',')[0]!.trim();
  const identity = createHash('sha256').update(`${ip}\0${subject ?? ''}`).digest('base64url');
  try {
    return await coordinator.allowRateLimit(`${group}:${identity}`, limit, windowMs);
  } catch (error) {
    console.warn('rate limiter unavailable', { group, error_name: error instanceof Error ? error.name : 'unknown' });
    return false;
  }
}
