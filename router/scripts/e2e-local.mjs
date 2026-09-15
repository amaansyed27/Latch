import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { testConfig } from '../dist/src/config.js';
import { MemoryCoordinator } from '../dist/src/memory-coordinator.js';
import { createRouterRuntime } from '../dist/src/runtime.js';

const temp = await mkdtemp(join(tmpdir(), 'latch-v05-e2e-'));
const workspace = join(temp, 'workspace');
const stateDir = join(temp, 'state');
const identityPath = join(temp, 'device.json');
const rootId = randomUUID();
const config = testConfig({ requestTimeoutMs: 10_000 });
const runtime = createRouterRuntime(config, new MemoryCoordinator());
let link = null;

try {
  await mkdir(workspace, { recursive: true });
  await mkdir(stateDir, { recursive: true });
  await writeFile(
    join(stateDir, 'local-config.json'),
    JSON.stringify(
      {
        paused: false,
        legacy_absolute_workspaces: false,
        permissions: {
          files: true,
          commands: true,
          screen: true,
          computer_control: false,
          mcp_discovery: false,
          mcp_execution: false,
        },
        roots: [
          {
            root_id: rootId,
            display_name: 'CI approved workspace',
            canonical_path: workspace,
          },
        ],
        mcp_servers: [],
      },
      null,
      2,
    ),
  );

  await new Promise((resolveListen) => runtime.server.listen(0, '127.0.0.1', resolveListen));
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
      LATCH_LOCAL_STATE_DIR: stateDir,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });

  const device = await waitForDevice(baseUrl, config.controlToken);

  const roots = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'roots-list',
    version: 2,
    method: 'roots.list',
    params: {},
  });
  assert.equal(roots.status, 'ok');
  assert.equal(roots.result?.type, 'roots');
  assert.deepEqual(roots.result?.data?.roots, [
    { root_id: rootId, display_name: 'CI approved workspace' },
  ]);

  const opened = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'workspace-open',
    version: 2,
    method: 'workspace.open',
    params: { root_id: rootId },
  });
  assert.equal(opened.status, 'ok');
  assert.equal(opened.result?.type, 'workspace');
  assert.equal(opened.result?.data?.root_id, rootId);
  assert.equal(opened.result?.data?.relative_path, '.');
  assert.equal(opened.result?.data?.developer_raw, false);
  assert.equal('root' in (opened.result?.data ?? {}), false);
  const workspaceId = opened.result?.data?.workspace_id;
  assert.equal(typeof workspaceId, 'string');

  const written = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'file-write',
    version: 2,
    method: 'fs.write',
    params: {
      workspace_id: workspaceId,
      path: 'proof.txt',
      contents: 'Latch V0.5 relay proof',
      overwrite: true,
    },
  });
  assert.equal(written.status, 'ok');
  assert.equal(written.result?.type, 'ack');

  const read = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'file-read',
    version: 2,
    method: 'fs.read',
    params: { workspace_id: workspaceId, path: 'proof.txt' },
  });
  assert.equal(read.status, 'ok');
  assert.equal(read.result?.data?.contents, 'Latch V0.5 relay proof');
  assert.equal(read.result?.data?.truncated, false);

  const exec = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'exec-run',
    version: 2,
    method: 'exec.run',
    params: {
      workspace_id: workspaceId,
      program: 'node',
      args: ['-e', "process.stdout.write('latch-remote-v05')"],
    },
  });
  assert.equal(exec.status, 'ok');
  assert.equal(exec.result?.data?.exit_code, 0);
  assert.equal(exec.result?.data?.stdout, 'latch-remote-v05');
  assert.equal(exec.result?.data?.timed_out, false);

  const computer = await sendRequest(baseUrl, config.controlToken, device.device_id, {
    id: 'computer-displays',
    version: 2,
    method: 'computer.displays',
    params: {},
  });
  assert.equal(computer.status, 'error');
  assert.equal(computer.error?.code, 'computer_unavailable');

  process.stdout.write('Latch V0.5 Router ↔ Link ↔ local engine E2E passed\n');
} catch (error) {
  process.stderr.write('Latch V0.5 relay E2E failed\n');
  throw error;
} finally {
  if (link !== null && link.exitCode === null) {
    link.kill('SIGINT');
    await Promise.race([
      new Promise((resolveExit) => link.once('exit', resolveExit)),
      new Promise((resolveTimeout) => setTimeout(resolveTimeout, 1_000)),
    ]);
    if (link.exitCode === null) link.kill('SIGKILL');
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
      if (body.devices?.length === 1) return body.devices[0];
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
  if (!response.ok) throw new Error(`Router request failed with HTTP ${response.status}`);
  return payload;
}
