import type { IncomingMessage } from 'node:http';

const buckets = new Map<string, { count: number; resetAt: number }>();

// ponytail: per-instance limiter; move counters to Redis if distributed abuse is observed.
export function allowRequest(request: IncomingMessage, group: string, limit: number, windowMs = 60_000): boolean {
  const ip = String(request.headers['x-forwarded-for'] ?? request.socket.remoteAddress ?? 'unknown').split(',')[0]!.trim();
  const key = `${group}:${ip}`;
  const now = Date.now();
  const current = buckets.get(key);
  if (!current || current.resetAt <= now) { buckets.set(key, { count: 1, resetAt: now + windowMs }); return true; }
  current.count += 1;
  return current.count <= limit;
}
