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
const MCP_SERVER_ID = '00000000-0000-4000-8000-000000000004';

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

void test('MCP initializes, advertises the V0.5 stable tool surface, and rejects missing auth', async () => {
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
    const names = tools.map((tool) => tool.name);
    for (const required of [
      'latch_devices_list',
      'latch_roots_list',
      'latch_workspace_open',
      'latch_files_list',
      'latch_file_stat',
      'latch_file_read',
      'latch_file_write',
      'latch_file_patch',
      'latch_files_search',
      'latch_directory_create',
      'latch_file_move',
      'latch_file_delete',
      'latch_exec_run',
      'latch_exec_start',
      'latch_exec_poll',
      'latch_exec_stdin',
      'latch_exec_kill',
      'latch_computer_displays',
      'latch_computer_screenshot',
      'latch_computer_windows',
      'latch_computer_focus',
      'latch_computer_mouse_move',
      'latch_computer_mouse_click',
      'latch_computer_mouse_drag',
      'latch_computer_scroll',
      'latch_computer_key',
      'latch_computer_type',
      'latch_mcp_servers_list',
      'latch_mcp_tools_list',
      'latch_mcp_call',
    ]) {
      assert(names.includes(required), `missing MCP tool ${required}`);
    }
    assert.equal(new Set(names).size, names.length);
    const deviceTool = tools.find((tool) => tool.name === 'latch_devices_list');
    const execTool = tools.find((tool) => tool.name === 'latch_exec_run');
    assert.equal(deviceTool?.annotations?.readOnlyHint, true);
    assert.equal(execTool?.annotations?.readOnlyHint, false);
    assert.equal(execTool?.annotations?.destructiveHint, true);
    assert.equal(execTool?.inputSchema.required?.includes('device_id'), true);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('MCP tools relay protocol v2 approved-root filesystem and concurrent execution requests', async () => {
  const router = await startMcpRouter();
  const methods: string[] = [];
  await registerDevice(router.coordinator, async (message) => {
    methods.push(message.request.method);
    assert.equal(message.request.version, 2);
    if (message.request.method === 'roots.list') {
      return response(message.request.id, 'roots', {
        roots: [{ root_id: ROOT_ID, display_name: 'Programming' }],
      });
    }
    if (message.request.method === 'workspace.open') {
      assert.deepEqual(message.request.params, { root_id: ROOT_ID, relative_path: 'project' });
      return response(message.request.id, 'workspace', {
        workspace_id: WORKSPACE_ID,
        root_id: ROOT_ID,
        display_name: 'Programming',
        relative_path: 'project',
        developer_raw: false,
      });
    }
    if (message.request.method === 'fs.read') {
      return response(message.request.id, 'file_content', { contents: 'safe text', truncated: false });
    }
    await new Promise((resolve) =>
      setTimeout(
        resolve,
        message.request.params && (message.request.params as { args?: string[] }).args?.[0] === 'slow' ? 20 : 1,
      ),
    );
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
    const roots = await client.callTool({
      name: 'latch_roots_list',
      arguments: { device_id: DEVICE_ID },
    });
    assert.deepEqual((roots.structuredContent as { roots: unknown[] }).roots, [
      { root_id: ROOT_ID, display_name: 'Programming' },
    ]);

    const opened = await client.callTool({
      name: 'latch_workspace_open',
      arguments: { device_id: DEVICE_ID, root_id: ROOT_ID, relative_path: 'project' },
    });
    assert.equal((opened.structuredContent as { workspace_id: string }).workspace_id, WORKSPACE_ID);
    assert.equal(JSON.stringify(opened.structuredContent).includes('C:\\'), false);

    const read = await client.callTool({
      name: 'latch_file_read',
      arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, relative_path: 'note.txt' },
    });
    assert.equal((read.structuredContent as { contents: string }).contents, 'safe text');

    const calls = await Promise.all([
      client.callTool({
        name: 'latch_exec_run',
        arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['slow'] },
      }),
      client.callTool({
        name: 'latch_exec_run',
        arguments: { device_id: DEVICE_ID, workspace_id: WORKSPACE_ID, program: 'node', args: ['fast'] },
      }),
    ]);
    assert.equal((calls[0]?.structuredContent as { stdout: string }).stdout, 'slow\n');
    assert.equal((calls[1]?.structuredContent as { stdout: string }).stdout, 'fast\n');
    assert.deepEqual(methods, ['roots.list', 'workspace.open', 'fs.read', 'exec.run', 'exec.run']);
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('screenshot results are returned as MCP image content without router persistence', async () => {
  const router = await startMcpRouter();
  await registerDevice(router.coordinator, async (message) => {
    assert.equal(message.request.method, 'computer.screenshot');
    return response(message.request.id, 'screenshot', {
      display_id: 'display-1',
      width: 2,
      height: 2,
      mime_type: 'image/png',
      data_base64: 'iVBORw0KGgo=',
    });
  });
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const result = await client.callTool({
      name: 'latch_computer_screenshot',
      arguments: { device_id: DEVICE_ID, display_id: 'display-1', format: 'png' },
    });
    const content = result.content as Array<Record<string, unknown>>;
    const image = content.find((block) => block.type === 'image');
    assert(image);
    assert.equal(image.mimeType, 'image/png');
    assert.equal(image.data, 'iVBORw0KGgo=');
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('local MCP results preserve text, image, and structured JSON content', async () => {
  const router = await startMcpRouter();
  await registerDevice(router.coordinator, async (message) => {
    assert.equal(message.request.method, 'mcp.call');
    return response(message.request.id, 'mcp_call', {
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
    const result = await client.callTool({
      name: 'latch_mcp_call',
      arguments: {
        device_id: DEVICE_ID,
        server_id: MCP_SERVER_ID,
        tool_name: 'create_object',
        arguments: { type: 'cube' },
      },
    });
    const content = result.content as Array<Record<string, unknown>>;
    assert(content.some((block) => block.type === 'text' && block.text === 'created cube'));
    assert(content.some((block) => block.type === 'image' && block.data === 'aW1hZ2U='));
    assert.deepEqual((result.structuredContent as { result: unknown }).result, {
      content: [
        { type: 'text', text: 'created cube' },
        { type: 'image', data: 'aW1hZ2U=', mimeType: 'image/png' },
      ],
      structuredContent: { object: 'Cube' },
    });
    await transport.close();
  } finally {
    await router.runtime.close();
  }
});

void test('MCP returns stable offline, timeout, workspace-expired, and validation errors', async () => {
  const router = await startMcpRouter(30);
  try {
    const { client, transport } = await connectClient(router.baseUrl, router.config.appToken);
    const offline = await client.callTool({
      name: 'latch_workspace_open',
      arguments: { device_id: DEVICE_ID, root_id: ROOT_ID },
    });
    assert.equal(toolError(offline).code, 'device_offline');

    await registerDevice(router.coordinator, async (message) => {
      if (message.request.method === 'fs.read') {
        return {
          kind: 'response',
          response: {
            id: message.request.id,
            version: 2,
            status: 'error',
            error: { code: 'workspace_expired', message: 'stale internal detail' },
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
    response: { id, version: 2, status: 'ok', result: { type, data } },
  };
}
