import assert from 'node:assert/strict';
import test from 'node:test';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

import { testConfig } from '../src/config.js';
import { MemoryCoordinator } from '../src/memory-coordinator.js';
import { createRouterRuntime } from '../src/runtime.js';
import type { DispatchMessage, RelayCompletion } from '../src/types.js';

const DEVICE_ID = '00000000-0000-4000-8000-000000000001';
const WORKSPACE_ID = '00000000-0000-4000-8000-000000000002';
const ROOT_ID = '00000000-0000-4000-8000-000000000003';
const SESSION_ID = '00000000-0000-4000-8000-000000000004';
const TAB_ID = '00000000-0000-4000-8000-000000000005';
const TOOL_REF = '00000000-0000-4000-8000-000000000006';

function toolError(result: unknown): { code: string; message: string } {
  const content = (result as { content: unknown }).content as { type: string; text: string }[];
  return (JSON.parse(content[0]!.text) as { error: { code: string; message: string } }).error;
}

async function startMcpRouter(timeoutMs = 200) {
  const config = testConfig({ requestTimeoutMs: timeoutMs });
  const coordinator = new MemoryCoordinator();
  const runtime = createRouterRuntime(config, coordinator);
  await runtime.ready;
  await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address();
  assert(address !== null && typeof address !== 'string');
  return { config, coordinator, runtime, baseUrl: `http://127.0.0.1:${address.port}` };
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

void test('MCP advertises exactly the nine V0.6 domain tools and rejects missing auth', async () => {
  const router = await startMcpRouter();
  try {
    const unauthorized = await fetch(`${router.baseUrl}/mcp`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }),
    });
    assert.equal(unauthorized.status, 401);
    assert(!((await unauthorized.text()).includes(router.config.appToken)));

    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const tools = (await client.listTools()).tools;
    const names = tools.map((tool) => tool.name).sort();
    assert.deepEqual(names, [
      'latch_act',
      'latch_browser',
      'latch_devices',
      'latch_events',
      'latch_exec',
      'latch_files',
      'latch_inspect',
      'latch_session',
      'latch_tools',
    ]);
    assert.equal(tools.find((tool) => tool.name === 'latch_devices')?.annotations?.readOnlyHint, true);
    assert.equal(tools.find((tool) => tool.name === 'latch_exec')?.annotations?.destructiveHint, true);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('domain tools relay protocol v3 agent requests and concurrent execution without primitive tool expansion', async () => {
  const router = await startMcpRouter();
  const calls: Array<{ domain: string; op: string }> = [];
  await registerDevice(router.coordinator, async (message) => {
    assert.equal(message.request.version, 3);
    assert.equal(message.request.method, 'agent');
    const params = message.request.params as { domain: string; request: Record<string, unknown> };
    calls.push({ domain: params.domain, op: String(params.request.op) });
    if (params.domain === 'session') return agentResponse(message.request.id, { session_id: SESSION_ID, state: 'active' });
    if (params.domain === 'files' && params.request.op === 'roots') {
      return agentResponse(message.request.id, { roots: [{ root_id: ROOT_ID, display_name: 'Programming' }] });
    }
    if (params.domain === 'files' && params.request.op === 'open_workspace') {
      return agentResponse(message.request.id, { workspace_id: WORKSPACE_ID, root_id: ROOT_ID, display_name: 'Programming', relative_path: 'project', developer_raw: false });
    }
    if (params.domain === 'files' && params.request.op === 'read') {
      return agentResponse(message.request.id, { contents: 'safe text', truncated: false });
    }
    const request = params.request as { args?: string[] };
    await new Promise((resolve) => setTimeout(resolve, request.args?.[0] === 'slow' ? 20 : 1));
    return agentResponse(message.request.id, { exit_code: 0, stdout: `${request.args?.[0]}\n`, stderr: '' });
  });

  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const session = await client.callTool({ name: 'latch_session', arguments: { device_id: DEVICE_ID, request: { op: 'create' } } });
    assert.equal((session.structuredContent as { session_id: string }).session_id, SESSION_ID);

    const roots = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_ID, request: { op: 'roots' } } });
    assert.equal((roots.structuredContent as { roots: unknown[] }).roots.length, 1);
    const opened = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_ID, request: { op: 'open_workspace', root_id: ROOT_ID, relative_path: 'project' } } });
    assert.equal((opened.structuredContent as { workspace_id: string }).workspace_id, WORKSPACE_ID);
    const read = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_ID, request: { op: 'read', workspace_id: WORKSPACE_ID, path: 'note.txt' } } });
    assert.equal((read.structuredContent as { contents: string }).contents, 'safe text');

    const results = await Promise.all([
      client.callTool({ name: 'latch_exec', arguments: { device_id: DEVICE_ID, request: { op: 'run', session_id: SESSION_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['slow'] } } }),
      client.callTool({ name: 'latch_exec', arguments: { device_id: DEVICE_ID, request: { op: 'run', session_id: SESSION_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['fast'] } } }),
    ]);
    assert.equal((results[0]?.structuredContent as { stdout: string }).stdout, 'slow\n');
    assert.equal((results[1]?.structuredContent as { stdout: string }).stdout, 'fast\n');
    assert.deepEqual(calls.map((call) => `${call.domain}:${call.op}`), ['session:create', 'files:roots', 'files:open_workspace', 'files:read', 'exec:run', 'exec:run']);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('V0.6 screenshot data is surfaced as MCP image content', async () => {
  const router = await startMcpRouter();
  await registerDevice(router.coordinator, async (message) => {
    const params = message.request.params as { domain: string; request: { op: string } };
    assert.equal(params.domain, 'browser');
    assert.equal(params.request.op, 'screenshot');
    return agentResponse(message.request.id, { width: 2, height: 2, mime_type: 'image/png', data_base64: 'iVBORw0KGgo=' });
  });
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const result = await client.callTool({ name: 'latch_browser', arguments: { device_id: DEVICE_ID, request: { op: 'screenshot', session_id: SESSION_ID, tab_id: TAB_ID } } });
    const image = (result.content as Array<Record<string, unknown>>).find((block) => block.type === 'image');
    assert.equal(image?.mimeType, 'image/png');
    assert.equal(image?.data, 'iVBORw0KGgo=');
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('lazy local MCP invocation preserves text, image, and structured output', async () => {
  const router = await startMcpRouter();
  await registerDevice(router.coordinator, async (message) => {
    const params = message.request.params as { domain: string; request: { op: string; tool_ref?: string } };
    assert.equal(params.domain, 'tools');
    assert.equal(params.request.op, 'call');
    assert.equal(params.request.tool_ref, TOOL_REF);
    return agentResponse(message.request.id, {
      result: {
        content: [
          { type: 'text', text: 'created cube' },
          { type: 'image', data: 'aW1hZ2U=', mimeType: 'image/png' },
        ],
        structuredContent: { object: 'Cube' },
      },
    });
  });
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const result = await client.callTool({ name: 'latch_tools', arguments: { device_id: DEVICE_ID, request: { op: 'call', session_id: SESSION_ID, tool_ref: TOOL_REF, arguments: { type: 'cube' } } } });
    const content = result.content as Array<Record<string, unknown>>;
    assert(content.some((block) => block.type === 'text' && block.text === 'created cube'));
    assert(content.some((block) => block.type === 'image' && block.data === 'aW1hZ2U='));
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('V0.6 MCP returns stable offline, timeout, local, and schema validation errors', async () => {
  const router = await startMcpRouter(30);
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const offline = await client.callTool({ name: 'latch_session', arguments: { device_id: DEVICE_ID, request: { op: 'create' } } });
    assert.equal(toolError(offline).code, 'device_offline');

    await registerDevice(router.coordinator, async (message) => {
      const params = message.request.params as { domain: string; request: { op: string } };
      if (params.domain === 'files' && params.request.op === 'read') {
        return { kind: 'response', response: { id: message.request.id, version: 3, status: 'error', error: { code: 'workspace_expired', message: 'stale internal detail' } } };
      }
      return new Promise<RelayCompletion>(() => undefined);
    });
    const stale = await client.callTool({ name: 'latch_files', arguments: { device_id: DEVICE_ID, request: { op: 'read', workspace_id: WORKSPACE_ID, path: 'note.txt' } } });
    assert.equal(toolError(stale).message, 'Workspace is no longer open. Open the approved workspace again.');
    const timedOut = await client.callTool({ name: 'latch_exec', arguments: { device_id: DEVICE_ID, request: { op: 'run', session_id: SESSION_ID, workspace_id: WORKSPACE_ID, program: 'node', args: [] } } });
    assert.equal(toolError(timedOut).code, 'request_timeout');
    const invalid = await client.callTool({ name: 'latch_files', arguments: { device_id: 'invalid', request: { op: 'roots' } } });
    assert.equal(invalid.isError, true);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

function agentResponse(id: string, data: Record<string, unknown>): RelayCompletion {
  return { kind: 'response', response: { id, version: 3, status: 'ok', result: { type: 'agent', data } } };
}
