import assert from 'node:assert/strict';
import test from 'node:test';

import { MemoryAuthorizationStore } from '../src/authorization-store.js';
import { testConfig } from '../src/config.js';
import { MemoryCoordinator } from '../src/memory-coordinator.js';
import { createRouterRuntime } from '../src/runtime.js';

void test('web shell is never cached and missing static assets fail cleanly', async () => {
  const config = testConfig();
  const runtime = createRouterRuntime(
    config,
    new MemoryCoordinator(),
    new MemoryAuthorizationStore(),
  );
  await runtime.ready;
  await new Promise<void>((resolve) => runtime.server.listen(0, '127.0.0.1', resolve));
  const address = runtime.server.address();
  assert(address && typeof address !== 'string');
  const base = `http://127.0.0.1:${address.port}`;
  config.publicBaseUrl = base;

  try {
    const page = await fetch(`${base}/download`);
    assert.equal(page.status, 200);
    assert.equal(page.headers.get('cache-control'), 'no-store');

    const missing = await fetch(`${base}/assets/definitely-missing-latch-chunk.js`);
    assert.equal(missing.status, 404);
    assert.match(missing.headers.get('content-type') ?? '', /^text\/plain/);
    assert.equal(missing.headers.get('cache-control'), 'no-store');
    assert.equal(await missing.text(), 'Not found');

    const theme = await fetch(`${base}/theme.js`);
    assert.equal(theme.status, 200);
    assert.equal(theme.headers.get('cache-control'), 'public, max-age=300');
  } finally {
    await runtime.close();
  }
});
