import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const temp = await mkdtemp(join(tmpdir(), 'latch-v06-engine-e2e-'));
const stateDir = join(temp, 'state');
const workspace = join(temp, 'workspace');
const sourceFixture = join(repo, 'fixtures', 'v0.6-developer-loop');
const scaleFixture = join(repo, 'router', 'scripts', 'fixtures', 'mcp-scale.mjs');
const daemonPath = join(repo, 'target', 'debug', process.platform === 'win32' ? 'latch-daemon.exe' : 'latch-daemon');
const bridgePath = join(repo, 'runtime', 'browser', 'bridge.mjs');
const rootId = randomUUID();
const mcpServerId = randomUUID();
let daemon;
let lines;
const pending = new Map();
const stderr = [];

try {
  await mkdir(stateDir, { recursive: true });
  await cp(sourceFixture, workspace, { recursive: true });
  await writeFile(
    join(stateDir, 'local-config.json'),
    JSON.stringify(
      {
        paused: false,
        legacy_absolute_workspaces: false,
        permissions: {
          files: true,
          commands: true,
          screen: false,
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
          ui_inspection: 'deny',
          ui_control: 'deny',
          screen_capture: 'deny',
          raw_input: 'deny',
          browser_isolated: 'allow',
          browser_authenticated: 'deny',
          clipboard_read: 'deny',
          clipboard_write: 'deny',
          mcp_discovery: 'allow',
          mcp_execution: 'allow',
          native_system_control: 'deny',
        },
        roots: [{ root_id: rootId, display_name: 'V0.6 engine E2E', canonical_path: workspace }],
        mcp_servers: [
          {
            server_id: mcpServerId,
            display_name: '250-tool scale fixture',
            transport: {
              transport: 'stdio',
              command: process.execPath,
              arguments: [scaleFixture],
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

  daemon = spawn(daemonPath, [], {
    cwd: repo,
    env: {
      ...process.env,
      LATCH_LOCAL_STATE_DIR: stateDir,
      LATCH_BROWSER_BRIDGE: bridgePath,
      LATCH_BROWSER_HEADLESS: '1',
      RUST_LOG: 'warn',
    },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  daemon.stderr.setEncoding('utf8');
  daemon.stderr.on('data', (chunk) => stderr.push(chunk));
  lines = createInterface({ input: daemon.stdout, crlfDelay: Infinity });
  lines.on('line', (line) => {
    let response;
    try {
      response = JSON.parse(line);
    } catch {
      return;
    }
    const waiter = response.id ? pending.get(response.id) : undefined;
    if (!waiter) return;
    pending.delete(response.id);
    waiter.resolve(response);
  });

  const session = await agent('session', { op: 'create' });
  const sessionId = session.session_id;
  assert.equal(typeof sessionId, 'string');

  const opened = await agent('files', { op: 'open_workspace', root_id: rootId });
  const workspaceId = opened.workspace_id;
  assert.equal(typeof workspaceId, 'string');
  await agent('session', { op: 'update', session_id: sessionId, workspace_ids: [workspaceId] });

  const initial = await agent('files', { op: 'read', workspace_id: workspaceId, path: 'app.js' });
  assert.match(initial.contents, /const animationFixed = false;/);

  const terminalCreated = await agent('exec', {
    op: 'terminal_create',
    session_id: sessionId,
    workspace_id: workspaceId,
    rows: 24,
    cols: 100,
  });
  const terminalId = terminalCreated.terminal.terminal_id;
  assert.equal(terminalCreated.route_used, 'conpty');

  const beforeTerminal = await agent('events', {
    session_id: sessionId,
    after_sequence: 0,
    types: [],
    wait_ms: 0,
    max_events: 100,
  });
  const port = await freePort();
  await agent('exec', {
    op: 'terminal_write',
    session_id: sessionId,
    terminal_id: terminalId,
    text: `node server.mjs ${port}\r\n`,
  });
  const terminalEvents = await agent('events', {
    session_id: sessionId,
    after_sequence: beforeTerminal.latest_sequence,
    types: ['terminal.output'],
    wait_ms: 5000,
    max_events: 20,
  });
  assert(terminalEvents.events.some((event) => event.type === 'terminal.output'));

  const terminalRead = await retry(async () => {
    const result = await agent('exec', {
      op: 'terminal_read',
      session_id: sessionId,
      terminal_id: terminalId,
      max_bytes: 65536,
    });
    return result.snapshot.logical_screen.includes(`127.0.0.1:${port}`) ? result : null;
  });
  assert.match(terminalRead.snapshot.logical_screen, /Latch V0\.6 fixture listening/);

  // Unrelated calls must not destroy persistent terminal identity/state.
  await agent('files', { op: 'stat', workspace_id: workspaceId, path: 'index.html' });
  const terminals = await agent('exec', { op: 'terminal_list', session_id: sessionId });
  assert(terminals.terminals.some((terminal) => terminal.terminal_id === terminalId && terminal.state.state === 'running'));

  const contextCreated = await agent('browser', {
    op: 'create_context',
    session_id: sessionId,
    authenticated: false,
    persistent: false,
  });
  const contextId = contextCreated.context.context_id;
  const tabCreated = await agent('browser', {
    op: 'new_tab',
    session_id: sessionId,
    context_id: contextId,
    url: `http://127.0.0.1:${port}`,
  });
  const tabId = tabCreated.tab.tab_id;

  const brokenSnapshot = await agent('browser', { op: 'snapshot', session_id: sessionId, tab_id: tabId });
  assert.match(brokenSnapshot.snapshot.aria, /Landing animation: broken/);
  const found = await agent('browser', {
    op: 'find',
    session_id: sessionId,
    tab_id: tabId,
    target: { role: 'button', name: 'Run fixture action' },
    max_results: 5,
  });
  assert.equal(found.elements.length, 1);

  const beforeBrowser = await agent('events', {
    session_id: sessionId,
    after_sequence: 0,
    types: [],
    wait_ms: 0,
    max_events: 100,
  });
  const acted = await agent('browser', {
    op: 'act',
    session_id: sessionId,
    tab_id: tabId,
    target: { role: 'button', name: 'Run fixture action' },
    action: { kind: 'click' },
    browser_verification: { text_present: 'clicked' },
    verification: 'required',
  });
  assert.equal(acted.outcome, 'verified');

  const browserEvents = await agent('events', {
    session_id: sessionId,
    after_sequence: beforeBrowser.latest_sequence,
    types: ['browser.console', 'browser.network', 'browser.request_failed'],
    wait_ms: 1000,
    max_events: 100,
  });
  assert(browserEvents.events.some((event) => event.type === 'browser.console'));
  assert(browserEvents.events.some((event) => event.type === 'browser.network'));

  const consoleEntries = await agent('browser', {
    op: 'console',
    session_id: sessionId,
    tab_id: tabId,
    after_sequence: 0,
    max_entries: 100,
  });
  assert(consoleEntries.entries.some((entry) => entry.text.includes('latch-fixture-clicked')));
  assert(!consoleEntries.entries.some((entry) => ['error', 'pageerror'].includes(entry.kind)));
  const networkEntries = await agent('browser', {
    op: 'network',
    session_id: sessionId,
    tab_id: tabId,
    after_sequence: 0,
    max_entries: 100,
  });
  assert(networkEntries.entries.some((entry) => entry.text.includes('POST') && entry.text.includes('/api/ping')));
  const screenshot = await agent('browser', { op: 'screenshot', session_id: sessionId, tab_id: tabId });
  assert.equal(screenshot.screenshot.mime_type, 'image/jpeg');
  assert(screenshot.screenshot.data_base64.length > 1000);

  const patched = await agent('files', {
    op: 'patch',
    workspace_id: workspaceId,
    path: 'app.js',
    replacements: [{ old: 'const animationFixed = false;', new: 'const animationFixed = true;' }],
    verification: 'required',
  });
  assert.equal(patched.outcome, 'verified');
  const patchedSource = await readFile(join(workspace, 'app.js'), 'utf8');
  assert.match(patchedSource, /const animationFixed = true;/);

  await agent('browser', {
    op: 'navigate',
    session_id: sessionId,
    tab_id: tabId,
    url: `http://127.0.0.1:${port}`,
  });
  const fixedSnapshot = await agent('browser', { op: 'snapshot', session_id: sessionId, tab_id: tabId });
  assert.match(fixedSnapshot.snapshot.aria, /Landing animation: fixed/);

  const tests = await agent('exec', {
    op: 'run',
    session_id: sessionId,
    workspace_id: workspaceId,
    program: process.execPath,
    args: ['verify-fixed.mjs'],
  });
  assert.equal(tests.exit_code, 0);
  assert.match(tests.stdout, /fix verified/);

  // Live MCP scale fixture: 250 advertised tools, bounded search, lazy describe,
  // invocation, and persistent stdio connection reuse proven by stable child PID.
  const providersBefore = await agent('tools', { op: 'providers', session_id: sessionId });
  const scaleProvider = providersBefore.providers.find((provider) => provider.server_id === mcpServerId);
  assert(scaleProvider);
  assert.equal(scaleProvider.connected, true);
  assert.equal(scaleProvider.cached_tools, 250);
  const catalogueHash = scaleProvider.catalogue_hash;

  const searched = await agent('tools', {
    op: 'search',
    session_id: sessionId,
    query: 'benchmark',
    provider_id: mcpServerId,
    max_results: 5,
  });
  assert.equal(searched.tools.length, 5);
  assert(searched.tools.every((tool) => !Object.hasOwn(tool, 'input_schema')));
  const selected = searched.tools[0];
  const described = await agent('tools', {
    op: 'describe',
    session_id: sessionId,
    tool_ref: selected.tool_ref,
  });
  assert.equal(described.tool.tool_ref, selected.tool_ref);
  assert.equal(described.tool.input_schema.type, 'object');

  const firstCall = await agent('tools', {
    op: 'call',
    session_id: sessionId,
    tool_ref: selected.tool_ref,
    arguments: { value: 'first' },
  });
  const secondCall = await agent('tools', {
    op: 'call',
    session_id: sessionId,
    tool_ref: selected.tool_ref,
    arguments: { value: 'second' },
  });
  const firstStructured = structuredContent(firstCall.result);
  const secondStructured = structuredContent(secondCall.result);
  assert.equal(firstStructured.pid, secondStructured.pid);
  assert.equal(secondStructured.call_count, firstStructured.call_count + 1);

  const providersAfter = await agent('tools', { op: 'providers', session_id: sessionId });
  const scaleAfter = providersAfter.providers.find((provider) => provider.server_id === mcpServerId);
  assert.equal(scaleAfter.cached_tools, 250);
  assert.equal(scaleAfter.catalogue_hash, catalogueHash);
  assert.equal(scaleAfter.catalogue_version, 1);

  await agent('browser', { op: 'close_tab', session_id: sessionId, tab_id: tabId });
  await agent('browser', { op: 'close_context', session_id: sessionId, context_id: contextId });
  await agent('exec', { op: 'terminal_kill', session_id: sessionId, terminal_id: terminalId });
  await agent('session', { op: 'close', session_id: sessionId });

  process.stdout.write('Latch V0.6 engine developer-loop + MCP scale E2E passed\n');
} finally {
  for (const waiter of pending.values()) waiter.reject(new Error('daemon stopped'));
  pending.clear();
  lines?.close();
  if (daemon?.exitCode === null) {
    daemon.stdin.end();
    await Promise.race([
      new Promise((done) => daemon.once('exit', done)),
      new Promise((done) => setTimeout(done, 1000)),
    ]);
    if (daemon.exitCode === null) daemon.kill('SIGKILL');
  }
  await rm(temp, { recursive: true, force: true });
}

async function agent(domain, request) {
  const id = randomUUID();
  const responsePromise = new Promise((resolveResponse, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Latch daemon request timed out: ${domain}/${request.op}`));
    }, 30_000);
    pending.set(id, {
      resolve(response) {
        clearTimeout(timer);
        resolveResponse(response);
      },
      reject(error) {
        clearTimeout(timer);
        reject(error);
      },
    });
  });
  daemon.stdin.write(`${JSON.stringify({ id, version: 3, method: 'agent', params: { domain, request } })}\n`);
  const response = await responsePromise;
  if (response.status !== 'ok') {
    throw new Error(`Latch ${domain}/${request.op} failed: ${JSON.stringify(response.error)}\n${stderr.join('')}`);
  }
  assert.equal(response.result.type, 'agent');
  return response.result.data;
}

function structuredContent(result) {
  return result?.structuredContent ?? result?.structured_content ?? {};
}

async function retry(operation, attempts = 100) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const value = await operation();
    if (value) return value;
    await new Promise((done) => setTimeout(done, 50));
  }
  throw new Error('Timed out waiting for expected engine state');
}

async function freePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolveListen);
  });
  const address = server.address();
  assert(address && typeof address !== 'string');
  const port = address.port;
  await new Promise((resolveClose) => server.close(resolveClose));
  return port;
}
