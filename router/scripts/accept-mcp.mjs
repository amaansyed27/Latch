import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

for (const name of ['LATCH_ROUTER_URL', 'LATCH_APP_TOKEN']) {
  if (!process.env[name]) throw new Error(`${name} is required`);
}

const baseUrl = process.env.LATCH_ROUTER_URL.replace(/\/$/, '');
const directNode = spawnSync('node', ['--version'], { encoding: 'utf8' }).stdout.trim();
const identity = JSON.parse(await readFile(join(process.env.LOCALAPPDATA, 'Latch', 'device.json'), 'utf8'));
const workspace = await mkdtemp(join(tmpdir(), 'Latch-v03-acceptance-'));
await writeFile(join(workspace, 'proof.txt'), 'Latch V0.3 production MCP acceptance');
const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), {
  requestInit: { headers: { authorization: `Bearer ${process.env.LATCH_APP_TOKEN}` } },
});
const client = new Client({ name: 'latch-production-acceptance', version: '0.3.0' });

try {
  const healthResponse = await fetch(`${baseUrl}/api/health`);
  assert.equal(healthResponse.status, 200);
  assert.equal((await healthResponse.json()).status, 'ok');
  await client.connect(transport);
  const tools = await client.listTools();
  assert.deepEqual(tools.tools.map(({ name }) => name), [
    'latch_devices_list',
    'latch_workspace_open',
    'latch_file_read',
    'latch_exec_run',
  ]);
  const listed = await client.callTool({ name: 'latch_devices_list', arguments: {} });
  const devices = listed.structuredContent?.devices ?? [];
  const matches = devices.filter(({ device_id }) => device_id === identity.device_id);
  assert.equal(matches.length, 1, 'Persisted local device is not online');
  const deviceId = matches[0].device_id;
  const opened = await client.callTool({
    name: 'latch_workspace_open',
    arguments: { device_id: deviceId, path: workspace },
  });
  const workspaceId = opened.structuredContent?.workspace_id;
  assert.equal(typeof workspaceId, 'string');
  const read = await client.callTool({
    name: 'latch_file_read',
    arguments: { device_id: deviceId, workspace_id: workspaceId, relative_path: 'proof.txt' },
  });
  assert.equal(read.structuredContent?.contents, 'Latch V0.3 production MCP acceptance');
  const executed = await client.callTool({
    name: 'latch_exec_run',
    arguments: { device_id: deviceId, workspace_id: workspaceId, program: 'node', args: ['--version'] },
  });
  const routedNode = executed.structuredContent?.stdout?.trim();
  assert.equal(executed.structuredContent?.exit_code, 0);
  assert.equal(executed.structuredContent?.timed_out, false);
  assert.equal(routedNode, directNode);
  process.stdout.write(`${JSON.stringify({ health: 'ok', device_id: deviceId, local_node: directNode, routed_node: routedNode, file_read: 'passed' })}\n`);
} finally {
  await client.close();
  await unlink(join(workspace, 'proof.txt')).catch(() => undefined);
  await rm(workspace, { recursive: true, force: true }).catch((error) => {
    if (process.platform !== 'win32' || error?.code !== 'EBUSY') throw error;
    process.stderr.write('Empty acceptance workspace remains locked until Latch Link exits.\n');
  });
}
