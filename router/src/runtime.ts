import { createServer, type Server } from 'node:http';

import type { RelayCoordinator } from './coordinator.js';
import type { RouterConfig } from './config.js';
import { createHttpHandler } from './http-api.js';
import { LinkServer } from './link-server.js';
import { createMcpHandler } from './mcp-server.js';

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
  const httpHandler = createHttpHandler(config, coordinator, () => linkServer.ready);
  const mcpHandler = createMcpHandler(config, coordinator, () => linkServer.ready);
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://router.local');
    if (url.pathname === '/mcp' || url.searchParams.has('latch_mcp')) {
      mcpHandler(request, response);
    } else {
      httpHandler(request, response);
    }
  });
  linkServer.attach(server);

  return {
    server,
    get ready(): Promise<void> {
      return linkServer.ready;
    },
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
