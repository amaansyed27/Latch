import { randomUUID } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';

import { isControlAuthorized } from './auth.js';
import type { RelayCoordinator } from './coordinator.js';
import type { RouterConfig } from './config.js';
import type { PublicDevice, RelayError } from './types.js';
import { isLatchRequestEnvelope, isUuid } from './validation.js';

const MAX_REQUEST_BODY_BYTES = 256 * 1024;

export function createHttpHandler(
  config: RouterConfig,
  coordinator: RelayCoordinator,
  ready: () => Promise<void>,
): (request: IncomingMessage, response: ServerResponse) => void {
  return (request, response) => {
    void handle(request, response, config, coordinator, ready).catch((error: unknown) => {
      console.error('router request failed', {
        error_name: error instanceof Error ? error.name : 'unknown',
      });
      sendJson(response, 503, {
        error: {
          code: 'router_unavailable',
          message: 'the router relay is temporarily unavailable',
        },
      });
    });
  };
}

async function handle(
  request: IncomingMessage,
  response: ServerResponse,
  config: RouterConfig,
  coordinator: RelayCoordinator,
  ready: () => Promise<void>,
): Promise<void> {
  const path = routePath(request);

  if (request.method === 'GET' && path === '/api/health') {
    await ready();
    sendJson(response, 200, {
      status: 'ok',
      transport: 'websocket+redis',
      protocol_version: 1,
    });
    return;
  }

  if (!isControlAuthorized(request.headers, config.controlToken)) {
    sendJson(response, 401, {
      error: { code: 'unauthorized', message: 'authentication required' },
    });
    return;
  }

  await ready();

  if (request.method === 'GET' && path === '/api/devices') {
    const devices = (await coordinator.listDevices()).map<PublicDevice>((device) => ({
      device_id: device.device_id,
      device_name: device.device_name,
      status: device.status,
      connected_at: device.connected_at,
    }));
    sendJson(response, 200, { devices });
    return;
  }

  const match = /^\/api\/devices\/([^/]+)\/request$/.exec(path);
  if (request.method === 'POST' && match !== null) {
    const deviceId = decodeURIComponent(match[1] ?? '');
    if (!isUuid(deviceId)) {
      sendRelayError(response, 400, {
        code: 'invalid_device_id',
        message: 'device id is not a valid UUID',
      });
      return;
    }

    const device = await coordinator.getDevice(deviceId);
    if (device === null) {
      sendRelayError(response, 404, {
        code: 'device_offline',
        message: 'the requested device is not connected',
      });
      return;
    }

    const body = await readJsonBody(request);
    if (!isLatchRequestEnvelope(body)) {
      sendRelayError(response, 400, {
        code: 'invalid_request',
        message: 'body is not a valid Latch request envelope',
      });
      return;
    }

    const requestId = randomUUID();
    const completion = await coordinator.request(
      device.instance_id,
      {
        request_id: requestId,
        device_id: deviceId,
        request: body,
      },
      config.requestTimeoutMs,
    );
    response.setHeader('x-latch-request-id', requestId);

    if (completion.kind === 'response') {
      sendJson(response, 200, completion.response);
      return;
    }

    sendRelayError(response, relayStatus(completion.error.code), completion.error);
    return;
  }

  sendJson(response, 404, {
    error: { code: 'not_found', message: 'route not found' },
  });
}

function routePath(request: IncomingMessage): string {
  const url = new URL(request.url ?? '/', 'http://router.local');
  const rewritten = url.searchParams.get('latch_path');
  if (rewritten !== null && rewritten.length > 0) {
    return `/api/${rewritten.replace(/^\/+/, '')}`;
  }
  return url.pathname;
}

async function readJsonBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Uint8Array);
    size += buffer.length;
    if (size > MAX_REQUEST_BODY_BYTES) {
      return null;
    }
    chunks.push(buffer);
  }
  if (chunks.length === 0) {
    return null;
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString('utf8')) as unknown;
  } catch {
    return null;
  }
}

function relayStatus(code: string): number {
  switch (code) {
    case 'request_timeout':
      return 504;
    case 'device_offline':
    case 'device_disconnected':
      return 409;
    default:
      return 502;
  }
}

function sendRelayError(
  response: ServerResponse,
  status: number,
  error: RelayError,
): void {
  sendJson(response, status, { error });
}

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  if (response.headersSent) {
    return;
  }
  response.statusCode = status;
  response.setHeader('content-type', 'application/json; charset=utf-8');
  response.setHeader('cache-control', 'no-store');
  response.end(JSON.stringify(body));
}
