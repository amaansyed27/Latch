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
const temp = await mkdtemp(join(tmpdir(), 'latch-v06-mcp-e2e-'));
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
  await writeFile(join(workspace, 'seed.txt'), 'Latch V0.6 MCP seed');
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
        permission_policy_version: 1,
        capability_policy: {
          files_read: 'allow',
          files_write: 'allow',
          exec: 'allow',
          terminal: 'allow',
          application_control: 'deny',
          ui_inspection: 'allow',
          ui_control: 'deny',
          screen_capture: 'allow',
          raw_input: 'deny',
          browser_isolated: 'deny',
          browser_authenticated: 'deny',
          clipboard_read: 'deny',
          clipboard_write: 'deny',
          mcp_discovery: 'allow',
          mcp_execution: 'allow',
          native_system_control: 'deny',
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
      LATCH_DEVICE_NAME: 'mcp-v06-e2e-device',
      LATCH_DEVICE_ID_PATH: identityPath,
      LATCH_LOCAL_STATE_DIR: stateDir,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });

  const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), {
    requestInit: { headers: { authorization: `Bearer ${config.appToken}` } },
  });
  client = new Client({ name: 'latch-e2e', version: '0.6.0' });
  await client.connect(transport);

  const device = await waitForDevice(client);

  const createdSession = await client.callTool({
    name: 'latch_session',
    arguments: { device_id: device.device_id, request: { op: 'create' } },
  });
  const sessionId = createdSession.structuredContent?.session_id;
  assert.equal(typeof sessionId, 'string');

  const roots = await client.callTool({
    name: 'latch_files',
    arguments: { device_id: device.device_id, request: { op: 'roots' } },
  });
  assert.deepEqual(roots.structuredContent?.roots, [
    { root_id: rootId, display_name: 'MCP E2E workspace' },
  ]);

  const opened = await client.callTool({
    name: 'latch_files',
    arguments: {
      device_id: device.device_id,
      request: { op: 'open_workspace', root_id: rootId },
    },
  });
  const workspaceId = opened.structuredContent?.workspace_id;
  assert.equal(typeof workspaceId, 'string');
  assert.equal(opened.structuredContent?.root_id, rootId);
  assert.equal(opened.structuredContent?.relative_path, '.');
  assert.equal(opened.structuredContent?.developer_raw, false);

  const bound = await client.callTool({
    name: 'latch_session',
    arguments: {
      device_id: device.device_id,
      request: { op: 'update', session_id: sessionId, workspace_ids: [workspaceId] },
    },
  });
  assert.equal(bound.structuredContent?.session_id, sessionId);
  assert(bound.structuredContent?.workspace_ids?.includes(workspaceId));

  const seed = await client.callTool({
    name: 'latch_files',
    arguments: {
      device_id: device.device_id,
      request: { op: 'read', workspace_id: workspaceId, path: 'seed.txt' },
    },
  });
  assert.equal(seed.structuredContent?.contents, 'Latch V0.6 MCP seed');

  const written = await client.callTool({
    name: 'latch_files',
    arguments: {
      device_id: device.device_id,
      request: {
        op: 'write',
        workspace_id: workspaceId,
        path: 'written-by-mcp.txt',
        contents: 'written through ChatGPT-facing MCP',
        overwrite: true,
      },
    },
  });
  assert.equal(written.isError, undefined);

  const readBack = await client.callTool({
    name: 'latch_files',
    arguments: {
      device_id: device.device_id,
      request: { op: 'read', workspace_id: workspaceId, path: 'written-by-mcp.txt' },
    },
  });
  assert.equal(readBack.structuredContent?.contents, 'written through ChatGPT-facing MCP');

  const executed = await client.callTool({
    name: 'latch_exec',
    arguments: {
      device_id: device.device_id,
      request: {
        op: 'run',
        session_id: sessionId,
        workspace_id: workspaceId,
        program: 'node',
        args: ['--version'],
      },
    },
  });
  assert.equal(executed.structuredContent?.exit_code, 0);
  assert.equal(executed.structuredContent?.timed_out, false);
  assert.equal(executed.structuredContent?.stdout.trim(), directNode);

  const windows = await client.callTool({
    name: 'latch_inspect',
    arguments: {
      device_id: device.device_id,
      request: { op: 'windows', session_id: sessionId },
    },
  });
  assert.equal(windows.isError, true);
  assert.equal(toolError(windows).code, 'computer_unavailable');

  const providers = await client.callTool({
    name: 'latch_tools',
    arguments: {
      device_id: device.device_id,
      request: { op: 'providers', session_id: sessionId },
    },
  });
  assert.equal(providers.isError, undefined);
  assert.equal(providers.structuredContent?.providers?.length, 1);
  assert.equal(providers.structuredContent?.providers?.[0]?.server_id, localMcpId);
  assert.equal(providers.structuredContent?.providers?.[0]?.display_name, 'CI stdio MCP');
  assert.equal(providers.structuredContent?.providers?.[0]?.connected, true);

  const localTools = await client.callTool({
    name: 'latch_tools',
    arguments: {
      device_id: device.device_id,
      request: {
        op: 'search',
        session_id: sessionId,
        query: 'echo',
        provider_id: localMcpId,
        max_results: 5,
      },
    },
  });
  assert.equal(localTools.structuredContent?.tools?.length, 1);
  assert.equal(localTools.structuredContent?.tools?.[0]?.name, 'echo');
  const toolRef = localTools.structuredContent?.tools?.[0]?.tool_ref;
  assert.equal(typeof toolRef, 'string');

  const described = await client.callTool({
    name: 'latch_tools',
    arguments: {
      device_id: device.device_id,
      request: { op: 'describe', session_id: sessionId, tool_ref: toolRef },
    },
  });
  assert.equal(described.structuredContent?.tool?.name, 'echo');
  assert.equal(described.structuredContent?.tool?.provider_id, localMcpId);

  const localCall = await client.callTool({
    name: 'latch_tools',
    arguments: {
      device_id: device.device_id,
      request: {
        op: 'call',
        session_id: sessionId,
        tool_ref: toolRef,
        arguments: { value: 'hello-through-latch' },
      },
    },
  });
  assert.equal(localCall.isError, undefined);
  assert(
    localCall.content.some(
      (block) => block.type === 'text' && block.text === 'local-mcp:hello-through-latch',
    ),
  );

  const closed = await client.callTool({
    name: 'latch_session',
    arguments: {
      device_id: device.device_id,
      request: { op: 'close', session_id: sessionId },
    },
  });
  assert.equal(closed.structuredContent?.session_id, sessionId);

  process.stdout.write(`Latch V0.6 MCP local E2E passed (${directNode})\n`);
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
    const result = await mcpClient.callTool({ name: 'latch_devices', arguments: {} });
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
