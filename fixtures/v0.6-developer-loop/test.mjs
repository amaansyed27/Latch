import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { server } from './server.mjs';

const root = dirname(fileURLToPath(import.meta.url));

void test('fixture begins in the documented deterministic defect state', async () => {
  const source = await readFile(join(root, 'app.js'), 'utf8');
  assert.match(source, /const animationFixed = false;/);
  const html = await readFile(join(root, 'index.html'), 'utf8');
  assert.match(html, /data-animation-state="broken"/);
});

void test('fixture exposes deterministic page and network action', async (t) => {
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const address = server.address();
  assert(address && typeof address !== 'string');
  const base = `http://127.0.0.1:${address.port}`;

  const page = await fetch(`${base}/`);
  assert.equal(page.status, 200);
  assert.match(await page.text(), /Latch developer-loop fixture/);

  const ping = await fetch(`${base}/api/ping`, { method: 'POST' });
  assert.equal(ping.status, 200);
  const payload = await ping.json();
  assert.equal(payload.status, 'clicked');
  assert.equal(typeof payload.request_id, 'string');
  assert(payload.request_id.length > 10);
});
