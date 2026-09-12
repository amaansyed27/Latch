import { createServer, type Server } from 'node:http';

import type { RelayCoordinator } from './coordinator.js';
import type { RouterConfig } from './config.js';
import { createHttpHandler } from './http-api.js';
import { LinkServer } from './link-server.js';

export interface RouterRuntime {
  server: Server;
  ready: Promise<void>;
  close(): Promise<void>;
}

export function createRouterRuntime(
  config: RouterConfig,
  coordinator: RelayCoordinator,
): RouterRuntime {
  const linkServer = new LinkServer(config, coordinator);
  const server = createServer(
    createHttpHandler(config, coordinator, linkServer.ready),
  );
  linkServer.attach(server);

  return {
    server,
    ready: linkServer.ready,
    async close(): Promise<void> {
      await linkServer.close();
      if (server.listening) {
        await new Promise<void>((resolve, reject) => {
          server.close((error) => {
            if (error === undefined) {
              resolve();
            } else {
              reject(error);
            }
          });
        });
      }
    },
  };
}
