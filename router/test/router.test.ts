import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';

import WebSocket from 'ws';

import { testConfig, type RouterConfig } from '../src/config.js';
import { MemoryCoordinator } from '../src/memory-coordinator.js';
import {
  createRouterRuntime,
  type RouterRuntime,
} from '../src/runtime.js';

type JsonObject = Record<string, unknown>;

interface TestRouter {
  runtime: RouterRuntime;
  baseUrl: string;
  config: RouterConfig;
}

async function startRouter(
  overrides: Partial<RouterConfig> = {},
): Promise<TestRouter> {
  const config = testConfig(overrides);
  const runtime = createRouterRuntime(config, new MemoryCoordinator());
  await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  await runtime.ready;
  const address = runtime.server.address();
  assert(address !== null && typeof address === 'object');
  return {
    runtime,
    baseUrl: `http://127.0.0.1:${address.port}`,
    config,
  };
}

async function stopRouter(router: TestRouter): Promise<void> {
  await router.runtime.close();
}

function controlHeaders(router: TestRouter, token = router.config.controlToken) {
  return { authorization: `Bearer ${token}`, 'content-type': 'application/json' };
}

async function connectDevice(
  router: TestRouter,
  options: { deviceId?: string; token?: string; deviceName?: string } = {},
): Promise<{ ws: WebSocket; deviceId: string }> {
  const deviceId = options.deviceId ?? randomUUID();
  const ws = new WebSocket(router.baseUrl.replace(/^http/, 'ws') + '/api/link');
  await new Promise<void>((resolve, reject) => {
    ws.once('open', resolve);
    ws.once('error', reject);
  });
  ws.send(
    JSON.stringify({
      type: 'hello',
      device_id: deviceId,
      device_name: options.deviceName ?? 'test-device',
      pairing_token: options.token ?? router.config.pairingToken,
    }),
  );
  const welcome = await nextJson(ws);
  assert.deepEqual(welcome, { type: 'welcome', device_id: deviceId });
  return { ws, deviceId };
}

function nextJson(ws: WebSocket): Promise<JsonObject> {
  return new Promise((resolve, reject) => {
    ws.once('message', (data) => {
      try {
        resolve(JSON.parse(data.toString()) as JsonObject);
      } catch (error) {
        reject(error);
      }
    });
    ws.once('error', reject);
  });
}

async function requestDevice(
  router: TestRouter,
  deviceId: string,
  body: JsonObject,
): Promise<Response> {
  return fetch(`${router.baseUrl}/api/devices/${deviceId}/request`, {
    method: 'POST',
    headers: controlHeaders(router),
    body: JSON.stringify(body),
  });
}

function latchRequest(id: string): JsonObject {
  return {
    id,
    version: 1,
    method: 'exec.run',
    params: { workspace_id: randomUUID(), program: 'node', args: ['--version'] },
  };
}

void test('health endpoint is public and reports the relay transport', async () => {
  const router = await startRouter();
  try {
    const response = await fetch(`${router.baseUrl}/api/health`);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      status: 'ok',
      transport: 'websocket+redis',
      protocol_version: 1,
    });
  } finally {
    await stopRouter(router);
  }
});

void test('control API rejects unauthenticated requests without leaking tokens', async () => {
  const router = await startRouter();
  try {
    const badToken = 'definitely-wrong-control-token';
    const response = await fetch(`${router.baseUrl}/api/devices`, {
      headers: controlHeaders(router, badToken),
    });
    assert.equal(response.status, 401);
    const text = await response.text();
    assert(!text.includes(badToken));
    assert(!text.includes(router.config.controlToken));
    assert(!text.includes(router.config.pairingToken));
  } finally {
    await stopRouter(router);
  }
});

void test('device registration appears online and disappears after disconnect', async () => {
  const router = await startRouter();
  try {
    const { ws, deviceId } = await connectDevice(router, { deviceName: 'amaan-laptop' });
    const online = await fetch(`${router.baseUrl}/api/devices`, {
      headers: controlHeaders(router),
    });
    const body = (await online.json()) as { devices: JsonObject[] };
    assert.equal(body.devices.length, 1);
    assert.equal(body.devices[0]?.device_id, deviceId);
    assert.equal(body.devices[0]?.status, 'online');
    assert.equal(body.devices[0]?.device_name, 'amaan-laptop');

    ws.close();
    await waitFor(async () => {
      const response = await fetch(`${router.baseUrl}/api/devices`, {
        headers: controlHeaders(router),
      });
      const devices = ((await response.json()) as { devices: JsonObject[] }).devices;
      return devices.length === 0;
    });
  } finally {
    await stopRouter(router);
  }
});

void test('request routing correlates concurrent responses even when returned out of order', async () => {
  const router = await startRouter();
  try {
    const { ws, deviceId } = await connectDevice(router);
    const received: JsonObject[] = [];
    ws.on('message', (data) => {
      const message = JSON.parse(data.toString()) as JsonObject;
      if (message.type !== 'request') {
        return;
      }
      received.push(message);
      if (received.length === 2) {
        for (const item of [...received].reverse()) {
          const request = item.request as JsonObject;
          ws.send(
            JSON.stringify({
              type: 'response',
              request_id: item.request_id,
              response: {
                id: request.id,
                version: 1,
                status: 'ok',
                result: { type: 'ack' },
              },
            }),
          );
        }
      }
    });

    const [first, second] = await Promise.all([
      requestDevice(router, deviceId, latchRequest('first')),
      requestDevice(router, deviceId, latchRequest('second')),
    ]);
    assert.equal(first.status, 200);
    assert.equal(second.status, 200);
    assert.equal(((await first.json()) as JsonObject).id, 'first');
    assert.equal(((await second.json()) as JsonObject).id, 'second');
    ws.close();
  } finally {
    await stopRouter(router);
  }
});

void test('unknown devices and disconnected pending requests return structured errors', async () => {
  const router = await startRouter();
  try {
    const unknown = await requestDevice(router, randomUUID(), latchRequest('unknown'));
    assert.equal(unknown.status, 404);
    assert.equal(((await unknown.json()) as JsonObject).error !== undefined, true);

    const { ws, deviceId } = await connectDevice(router);
    const delivered = nextJson(ws);
    const pendingResponse = requestDevice(router, deviceId, latchRequest('disconnect'));
    const message = await delivered;
    assert.equal(message.type, 'request');
    ws.close();

    const disconnected = await pendingResponse;
    assert.equal(disconnected.status, 409);
    const error = ((await disconnected.json()) as JsonObject).error as JsonObject;
    assert.equal(error.code, 'device_disconnected');
  } finally {
    await stopRouter(router);
  }
});

void test('request timeout clears pending state and returns a gateway timeout', async () => {
  const router = await startRouter({ requestTimeoutMs: 75 });
  try {
    const { ws, deviceId } = await connectDevice(router);
    const response = await requestDevice(router, deviceId, latchRequest('timeout'));
    assert.equal(response.status, 504);
    const error = ((await response.json()) as JsonObject).error as JsonObject;
    assert.equal(error.code, 'request_timeout');
    ws.close();
  } finally {
    await stopRouter(router);
  }
});

void test('malformed and unauthenticated device messages are rejected without token leakage', async () => {
  const router = await startRouter();
  try {
    const ws = new WebSocket(router.baseUrl.replace(/^http/, 'ws') + '/api/link');
    await new Promise<void>((resolve, reject) => {
      ws.once('open', resolve);
      ws.once('error', reject);
    });
    ws.send('{bad-json');
    const malformed = JSON.stringify(await nextJson(ws));
    assert(malformed.includes('invalid_message'));
    assert(!malformed.includes(router.config.pairingToken));

    const bad = new WebSocket(router.baseUrl.replace(/^http/, 'ws') + '/api/link');
    await new Promise<void>((resolve, reject) => {
      bad.once('open', resolve);
      bad.once('error', reject);
    });
    const wrongToken = 'wrong-pairing-token';
    bad.send(
      JSON.stringify({
        type: 'hello',
        device_id: randomUUID(),
        device_name: 'bad-device',
        pairing_token: wrongToken,
      }),
    );
    const rejected = JSON.stringify(await nextJson(bad));
    assert(rejected.includes('unauthorized'));
    assert(!rejected.includes(wrongToken));
    assert(!rejected.includes(router.config.pairingToken));
  } finally {
    await stopRouter(router);
  }
});

async function waitFor(
  predicate: () => Promise<boolean>,
  attempts = 40,
): Promise<void> {
  for (let index = 0; index < attempts; index += 1) {
    if (await predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.fail('condition did not become true');
}
