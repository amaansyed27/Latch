import assert from 'node:assert/strict';
import { createServer } from 'node:net';
import test from 'node:test';

import { RedisCoordinator } from '../src/redis-coordinator.js';

void test('failed Redis startup is cleaned up and can be retried', async () => {
  let connections = 0;
  const server = createServer((socket) => {
    connections += 1;
    socket.destroy();
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  assert(address !== null && typeof address !== 'string');

  const coordinator = new RedisCoordinator(`redis://127.0.0.1:${address.port}`);
  await assert.rejects(coordinator.start());
  const firstAttemptConnections = connections;
  await assert.rejects(coordinator.start());
  assert(connections > firstAttemptConnections);

  await coordinator.stop();
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error === undefined ? resolve() : reject(error))),
  );
});
