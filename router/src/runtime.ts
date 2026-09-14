import { createServer, type Server } from 'node:http';

import type { RelayCoordinator } from './coordinator.js';
import type { RouterConfig } from './config.js';
import { createHttpHandler } from './http-api.js';
import { LinkServer } from './link-server.js';
import { createMcpHandler } from './mcp-server.js';
import { PostgresAuthorizationStore, type AuthorizationStore } from './authorization-store.js';
import { createOAuthHandler } from './oauth-server.js';

export interface RouterRuntime {
  server: Server;
  ready: Promise<void>;
  close(): Promise<void>;
}

export function createRouterRuntime(
  config: RouterConfig,
  coordinator: RelayCoordinator,
  authorizationStore?: AuthorizationStore,
): RouterRuntime {
  const store = authorizationStore ?? (config.databaseUrl ? new PostgresAuthorizationStore(config.databaseUrl) : undefined);
  const linkServer = new LinkServer(config, coordinator, undefined, store);
  const httpHandler = createHttpHandler(config, coordinator, () => linkServer.ready);
  const mcpHandler = createMcpHandler(config, coordinator, () => linkServer.ready, store);
  const oauthHandler = store ? createOAuthHandler(config, store, coordinator) : null;
  const server = createServer((request, response) => {
    let url = new URL(request.url ?? '/', 'http://router.local');
    const publicPath = url.searchParams.get('latch_public_path');
    if (publicPath !== null) {
      url.searchParams.delete('latch_public_path');
      request.url = `${publicPath}${url.search}`;
      url = new URL(request.url, 'http://router.local');
    }
    if (url.pathname === '/mcp' || url.searchParams.has('latch_mcp')) {
      mcpHandler(request, response);
    } else if (oauthHandler && (url.pathname.startsWith('/oauth/') || url.pathname.startsWith('/.well-known/') || url.pathname.startsWith('/api/auth/') || url.pathname.startsWith('/api/pairing/') || url.pathname.startsWith('/assets/') || url.pathname === '/api/my/devices' || ['/','/login','/signup','/account','/forgot-password','/reset-password','/devices','/download','/privacy','/terms','/support','/security'].includes(url.pathname) || (request.method === 'GET' && request.headers.accept?.includes('text/html')))) {
      oauthHandler(request, response);
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
