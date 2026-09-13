import assert from 'node:assert/strict';
import test from 'node:test';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { testConfig } from '../src/config.js';
import { MemoryCoordinator } from '../src/memory-coordinator.js';
import { createRouterRuntime } from '../src/runtime.js';

function toolError(result: unknown): { code: string; message: string } {
  const content = (result as { content: unknown }).content as { type: string; text: string }[];
  return (JSON.parse(content[0]!.text) as { error: { code: string; message: string } }).error;
}
import type { DispatchMessage, RelayCompletion } from '../src/types.js';

const DEVICE_ID = '00000000-0000-4000-8000-000000000001';
const WORKSPACE_ID = '00000000-0000-4000-8000-000000000002';

async function startMcpRouter(timeoutMs = 200) {
  const config = testConfig({ requestTimeoutMs: timeoutMs });
  const coordinator = new MemoryCoordinator();
  const runtime = createRouterRuntime(config, coordinator);
  await runtime.ready;
  await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address();
  assert(address !== null && typeof address !== 'string');
  const baseUrl = `http://127.0.0.1:${address.port}`;
  return { config, coordinator, runtime, baseUrl };
}

async function connectClient(baseUrl: string, token: string) {
  const client = new Client({ name: 'latch-test', version: '1.0.0' });
  const transport = new StreamableHTTPClientTransport(new URL(`${baseUrl}/mcp`), {
    requestInit: { headers: { authorization: `Bearer ${token}` } },
  });
  await client.connect(transport);
  return { client, transport };
}

async function registerDevice(
  coordinator: MemoryCoordinator,
  handler: (message: DispatchMessage) => RelayCompletion | Promise<RelayCompletion>,
) {
  await coordinator.registerDevice({
    device_id: DEVICE_ID,
    device_name: 'Windows laptop',
    status: 'online',
    connected_at: new Date().toISOString(),
    instance_id: 'device-instance',
    connection_id: 'device-connection',
  });
  return coordinator.subscribeDispatch('device-instance', async (message) => {
    await coordinator.respond(message.request_id, await handler(message));
  });
}

void test('MCP initializes, advertises bounded tools, and rejects missing auth', async () => {
  const router = await startMcpRouter();
  try {
    const unauthorized = await fetch(`${router.baseUrl}/mcp`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }),
    });
    assert.equal(unauthorized.status, 401);
    assert(!((await unauthorized.text()).includes(router.config.appToken)));

    const { client, transport } = await connectClient(
      router.baseUrl,
      router.config.appToken,
    );
    const tools = (await client.listTools()).tools;
    assert.deepEqual(
      tools.map((tool) => tool.name),
      ['latch_devices_list', 'latch_workspace_open', 'latch_file_read', 'latch_exec_run'],
    );
    assert.equal(tools[0]?.annotations?.readOnlyHint, true);
    assert.equal(tools[3]?.annotations?.readOnlyHint, false);
    assert.equal(tools[3]?.annotations?.destructiveHint, true);
    assert.equal(tools[3]?.inputSchema.required?.includes('device_id'), true);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('MCP tools translate to typed Latch requests and correlate concurrency', async () => {
  const router = await startMcpRouter();
  const methods: string[] = [];
  await registerDevice(router.coordinator, async (message) => {
    methods.push(message.request.method);
    if (message.request.method === 'workspace.open') {
      return response(message.request.id, 'workspace', {
        workspace_id: WORKSPACE_ID,
        root: 'C:\\safe',
      });
    }
    if (message.request.method === 'fs.read') {
      return response(message.request.id, 'file_content', { contents: 'safe text' });
    }
    await new Promise((resolve) => setTimeout(resolve, message.request.params && (message.request.params as { args?: string[] }).args?.[0] === 'slow' ? 20 : 1));
    return response(message.request.id, 'exec', {
      exit_code: 0,
      stdout: `${(message.request.params as { args: string[] }).args[0]}\n`,
      stderr: '',
      duration_ms: 1,
      timed_out: false,
      stdout_truncated: false,
      stderr_truncated: false,
    });
  });

  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const devices = await client.callTool({ name: 'latch_devices_list', arguments: {} });
    assert.equal((devices.structuredContent as { devices: unknown[] }).devices.length, 1);
    const opened = await client.callTool({
      name: 'latch_workspace_open',
      arguments: { device_id: DEVICE_ID, path: 'C:\\safe' },
    });
    assert.equal((opened.structuredContent as { workspace_id: string }).workspace_id, WORKSPACE_ID);
    const read = await client.callTool({
      name: 'latch_file_read',
      arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, relative_path: 'note.txt' },
    });
    assert.equal((read.structuredContent as { contents: string }).contents, 'safe text');
    const calls = await Promise.all([
      client.callTool({ name: 'latch_exec_run', arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['slow'] } }),
      client.callTool({ name: 'latch_exec_run', arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['fast'] } }),
    ]);
    assert.equal((calls[0]?.structuredContent as { stdout: string }).stdout, 'slow\n');
    assert.equal((calls[1]?.structuredContent as { stdout: string }).stdout, 'fast\n');
    assert.deepEqual(methods, ['workspace.open', 'fs.read', 'exec.run', 'exec.run']);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('MCP returns structured offline, timeout, stale-workspace, and input errors', async () => {
  const router = await startMcpRouter(30);
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const offline = await client.callTool({
      name: 'latch_workspace_open',
      arguments: { device_id: DEVICE_ID, path: 'C:\\safe' },
    });
    assert.equal(toolError(offline).code, 'device_not_found');

    await registerDevice(router.coordinator, async (message) => {
      if (message.request.method === 'fs.read') {
        return {
          kind: 'response',
          response: {
            id: message.request.id,
            version: 1,
            status: 'error',
            error: { code: 'workspace_not_found', message: 'stale internal detail' },
          },
        };
      }
      return new Promise<RelayCompletion>(() => undefined);
    });
    const stale = await client.callTool({
      name: 'latch_file_read',
      arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, relative_path: 'note.txt' },
    });
    assert.equal(toolError(stale).message, 'Workspace is no longer open. Call latch_workspace_open again.');
    const timedOut = await client.callTool({
      name: 'latch_exec_run',
      arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, program: 'node', args: [] },
    });
    assert.equal(toolError(timedOut).code, 'request_timeout');
    const invalid = await client.callTool({
      name: 'latch_file_read',
      arguments: { device_id: 'invalid' },
    });
    assert.equal(invalid.isError, true);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

function response(id: string, type: string, data: Record<string, unknown>): RelayCompletion {
  return {
    kind: 'response',
    response: { id, version: 1, status: 'ok', result: { type, data } },
  };
}
