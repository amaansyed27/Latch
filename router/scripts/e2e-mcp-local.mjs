import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { testConfig } from '../dist/src/config.js';
import { MemoryCoordinator } from '../dist/src/memory-coordinator.js';
import { createRouterRuntime } from '../dist/src/runtime.js';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const fixturePath = join(scriptDir, 'fixtures', 'local-mcp.mjs');
const temp = await mkdtemp(join(tmpdir(), 'latch-v05-mcp-e2e-'));
const workspace = join(temp, 'workspace');
const stateDir = join(temp, 'state');
const identityPath = join(temp, 'device.json');
const rootId = randomUUID();
const localMcpId = randomUUID();
const config = testConfig({ requestTimeoutMs: 15_000 });
const runtime = createRouterRuntime(config, new MemoryCoordinator());
const directNode = spawnSync('node', ['--version'], { encoding: 'utf8' }).stdout.trim();
let link;
let client;

try {
  await mkdir(workspace, { recursive: true });
  await mkdir(stateDir, { recursive: true });
  await writeFile(join(workspace, 'seed.txt'), 'Latch V0.5 MCP seed');
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
          mcp_discovery: true,
          mcp_execution: true,
        },
        roots: [
          {
            root_id: rootId,
            display_name: 'MCP E2E workspace',
            canonical_path: workspace,
          },
        ],
        mcp_servers: [
          {
            server_id: localMcpId,
            display_name: 'CI stdio MCP',
            transport: {
              transport: 'stdio',
              command: 'node',
              arguments: [fixturePath],
              environment_references: {},
            },
            enabled: true,
            allow_remote: true,
          },
        ],
      },
      null,
      2,
    ),
  );

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
      LATCH_DEVICE_NAME: 'mcp-v05-e2e-device',
      LATCH_DEVICE_ID_PATH: identityPath,
      LATCH_LOCAL_STATE_DIR: stateDir,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });

  const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), {
    requestInit: { headers: { authorization: `Bearer ${config.appToken}` } },
  });
  client = new Client({ name: 'latch-e2e', version: '0.5.0' });
  await client.connect(transport);

  const device = await waitForDevice(client);

  const roots = await client.callTool({
    name: 'latch_roots_list',
    arguments: { device_id: device.device_id },
  });
  assert.deepEqual(roots.structuredContent?.roots, [
    { root_id: rootId, display_name: 'MCP E2E workspace' },
  ]);

  const opened = await client.callTool({
    name: 'latch_workspace_open',
    arguments: { device_id: device.device_id, root_id: rootId },
  });
  const workspaceId = opened.structuredContent?.workspace_id;
  assert.equal(typeof workspaceId, 'string');
  assert.equal(opened.structuredContent?.root_id, rootId);
  assert.equal('root' in (opened.structuredContent ?? {}), false);

  const seed = await client.callTool({
    name: 'latch_file_read',
    arguments: {
      device_id: device.device_id,
      workspace_id: workspaceId,
      relative_path: 'seed.txt',
    },
  });
  assert.equal(seed.structuredContent?.contents, 'Latch V0.5 MCP seed');

  const written = await client.callTool({
    name: 'latch_file_write',
    arguments: {
      device_id: device.device_id,
      workspace_id: workspaceId,
      relative_path: 'written-by-mcp.txt',
      contents: 'written through ChatGPT-facing MCP',
      overwrite: true,
    },
  });
  assert.equal(written.isError, undefined);

  const readBack = await client.callTool({
    name: 'latch_file_read',
    arguments: {
      device_id: device.device_id,
      workspace_id: workspaceId,
      relative_path: 'written-by-mcp.txt',
    },
  });
  assert.equal(readBack.structuredContent?.contents, 'written through ChatGPT-facing MCP');

  const executed = await client.callTool({
    name: 'latch_exec_run',
    arguments: {
      device_id: device.device_id,
      workspace_id: workspaceId,
      program: 'node',
      args: ['--version'],
    },
  });
  assert.equal(executed.structuredContent?.exit_code, 0);
  assert.equal(executed.structuredContent?.timed_out, false);
  assert.equal(executed.structuredContent?.stdout.trim(), directNode);

  const displays = await client.callTool({
    name: 'latch_computer_displays',
    arguments: { device_id: device.device_id },
  });
  assert.equal(displays.isError, true);
  assert.equal(toolError(displays).code, 'computer_unavailable');

  const integrations = await client.callTool({
    name: 'latch_mcp_servers_list',
    arguments: { device_id: device.device_id },
  });
  assert.deepEqual(integrations.structuredContent?.servers, [
    { server_id: localMcpId, display_name: 'CI stdio MCP', status: 'stopped' },
  ]);

  const localTools = await client.callTool({
    name: 'latch_mcp_tools_list',
    arguments: { device_id: device.device_id, server_id: localMcpId },
  });
  assert.equal(localTools.structuredContent?.tools?.length, 1);
  assert.equal(localTools.structuredContent?.tools?.[0]?.name, 'echo');

  const localCall = await client.callTool({
    name: 'latch_mcp_call',
    arguments: {
      device_id: device.device_id,
      server_id: localMcpId,
      tool_name: 'echo',
      arguments: { value: 'hello-through-latch' },
    },
  });
  assert.equal(localCall.isError, undefined);
  assert(
    localCall.content.some(
      (block) => block.type === 'text' && block.text === 'local-mcp:hello-through-latch',
    ),
  );

  process.stdout.write(`Latch V0.5 MCP local E2E passed (${directNode})\n`);
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

function toolError(result) {
  const block = result.content.find((item) => item.type === 'text');
  assert(block && block.type === 'text');
  return JSON.parse(block.text).error;
}
