import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { testConfig } from '../dist/src/config.js';
import { MemoryCoordinator } from '../dist/src/memory-coordinator.js';
import { createRouterRuntime } from '../dist/src/runtime.js';

const temp = await mkdtemp(join(tmpdir(), 'latch-v03-e2e-'));
const identityPath = join(temp, 'device.json');
const config = testConfig({ requestTimeoutMs: 10_000 });
const runtime = createRouterRuntime(config, new MemoryCoordinator());
const directNode = spawnSync('node', ['--version'], { encoding: 'utf8' }).stdout.trim();
let link;
let client;

try {
  await writeFile(join(temp, 'proof.txt'), 'Latch V0.3 MCP proof');
  await new Promise((done) => runtime.server.listen(0, '127.0.0.1', done));
  await runtime.ready;
  const address = runtime.server.address();
  assert(address && typeof address === 'object');
  const baseUrl = `http://127.0.0.1:${address.port}`;
  link = spawn(process.env.LATCH_LINK_BIN ?? resolve('..', 'target', 'debug', 'latch-link'), [], {
    env: {
      ...process.env,
      LATCH_ROUTER_URL: baseUrl,
      LATCH_PAIRING_TOKEN: config.pairingToken,
      LATCH_DEVICE_NAME: 'mcp-e2e-device',
      LATCH_DEVICE_ID_PATH: identityPath,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });

  const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), {
    requestInit: { headers: { authorization: `Bearer ${config.appToken}` } },
  });
  client = new Client({ name: 'latch-e2e', version: '0.3.0' });
  await client.connect(transport);
  const device = await waitForDevice(client);
  const opened = await client.callTool({
    name: 'latch_workspace_open',
    arguments: { device_id: device.device_id, path: temp },
  });
  const workspaceId = opened.structuredContent?.workspace_id;
  assert.equal(typeof workspaceId, 'string');
  const read = await client.callTool({
    name: 'latch_file_read',
    arguments: { device_id: device.device_id, workspace_id: workspaceId, relative_path: 'proof.txt' },
  });
  assert.equal(read.structuredContent?.contents, 'Latch V0.3 MCP proof');
  const executed = await client.callTool({
    name: 'latch_exec_run',
    arguments: { device_id: device.device_id, workspace_id: workspaceId, program: 'node', args: ['--version'] },
  });
  assert.equal(executed.structuredContent?.exit_code, 0);
  assert.equal(executed.structuredContent?.timed_out, false);
  assert.equal(executed.structuredContent?.stdout.trim(), directNode);
  process.stdout.write(`Latch MCP local E2E passed (${directNode})\n`);
} finally {
  await client?.close();
  if (link?.exitCode === null) {
    link.kill('SIGINT');
    await Promise.race([
      new Promise((done) => link.once('exit', done)),
      new Promise((done) => setTimeout(done, 1_000)),
    ]);
    if (link.exitCode === null) link.kill('SIGKILL');
  }
  await runtime.close();
  await rm(temp, { recursive: true, force: true });
}

async function waitForDevice(mcpClient) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const result = await mcpClient.callTool({ name: 'latch_devices_list', arguments: {} });
    const devices = result.structuredContent?.devices ?? [];
    if (devices.length === 1) return devices[0];
    await new Promise((done) => setTimeout(done, 50));
  }
  throw new Error('Latch Link did not register with the MCP Router');
}
