import { loadProductionConfig } from '../src/config.js';
import { RedisCoordinator } from '../src/redis-coordinator.js';
import { createRouterRuntime, type RouterRuntime } from '../src/runtime.js';
import { createUnconfiguredServer } from '../src/unconfigured.js';

const loaded = loadProductionConfig(process.env);
let runtime: RouterRuntime | null = null;

const server = loaded.ok
  ? (() => {
      runtime = createRouterRuntime(
        loaded.config,
        new RedisCoordinator(loaded.config.redisUrl),
      );
      return runtime.server;
    })()
  : createUnconfiguredServer();

process.once('SIGTERM', () => {
  if (runtime !== null) {
    void runtime.close().catch(() => undefined);
  }
});

export default server;
