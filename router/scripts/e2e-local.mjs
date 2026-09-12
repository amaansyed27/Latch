import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { testConfig } from '../dist/src/config.js';
import { MemoryCoordinator } from '../dist/src/memory-coordinator.js';
import { createRouterRuntime } from '../dist/src/runtime.js';

const temp = await mkdtemp(join(tmpdir(), 'latch-v02-e2e-'));
const workspace = join(temp, 'workspace');
const identityPath = join(temp, 'device.json');
const config = testConfig({ requestTimeoutMs: 5_000 });
const runtime = createRouterRuntime(config, new MemoryCoordinator());
let link = null;

try {
  await new Promise((resolveListen) =>
    runtime.server.listen(0, '127.0.0.1', resolveListen),
  );
  await runtime.ready;
  const address = runtime.server.address();
  assert(address && typeof address === 'object');
  const baseUrl = `http://127.0.0.1:${address.port}`;

  const binary = process.env.LATCH_LINK_BIN ?? resolve('target/debug/latch-link');
  link = spawn(binary, [], {
    env: {
      ...process.env,
      LATCH_ROUTER_URL: baseUrl,
      LATCH_PAIRING_TOKEN: config.pairingToken,
      LATCH_DEVICE_NAME: 'ci-local-device',
      LATCH_DEVICE_ID_PATH: identityPath,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });

  const device = await waitForDevice(baseUrl, config.controlToken);
  const open = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'workspace-open',
    version: 1,
    method: 'workspace.create',
    params: { path: workspace },
  });
  assert.equal(open.status, 'ok');
  const workspaceId = open.result?.data?.workspace_id;
  assert.equal(typeof workspaceId, 'string');

  const exec = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'exec-run',
    version: 1,
    method: 'exec.run',
    params: {
      workspace_id: workspaceId,
      program: 'node',
      args: ['-e', "process.stdout.write('latch-remote')"],
    },
  });
  assert.equal(exec.status, 'ok');
  assert.equal(exec.result?.data?.exit_code, 0);
  assert.equal(exec.result?.data?.stdout, 'latch-remote');

  process.stdout.write('Latch Router ↔ Latch Link ↔ local exec.run E2E passed\n');
} catch (error) {
  process.stderr.write('Latch Link failed during E2E\n');
  throw error;
} finally {
  if (link !== null && link.exitCode === null) {
    link.kill('SIGINT');
    await Promise.race([
      new Promise((resolveExit) => link.once('exit', resolveExit)),
      new Promise((resolveTimeout) => setTimeout(resolveTimeout, 1_000)),
    ]);
    if (link.exitCode === null) {
      link.kill('SIGKILL');
    }
  }
  await runtime.close();
  await rm(temp, { recursive: true, force: true });
}

async function waitForDevice(baseUrl, token) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const response = await fetch(`${baseUrl}/api/devices`, {
      headers: { authorization: `Bearer ${token}` },
    });
    if (response.ok) {
      const body = await response.json();
      if (body.devices?.length === 1) {
        return body.devices[0];
      }
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
  }
  throw new Error('Latch Link did not register with the test Router');
}

async function sendRequest(baseUrl, token, deviceId, body) {
  const response = await fetch(`${baseUrl}/api/devices/${deviceId}/request`, {
    method: 'POST',
    headers: {
      authorization: `Bearer ${token}`,
      'content-type': 'application/json',
    },
    body: JSON.stringify(body),
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(`Router request failed with HTTP ${response.status}`);
  }
  return payload;
}
