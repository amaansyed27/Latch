import { createServer, type ServerResponse } from 'node:http';

export function createUnconfiguredServer() {
  return createServer((_request, response) => {
    sendJson(response, 503, {
      status: 'degraded',
      error: {
        code: 'router_not_configured',
        message: 'required router configuration is not available',
      },
    });
  });
}

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  response.statusCode = status;
  response.setHeader('content-type', 'application/json; charset=utf-8');
  response.setHeader('cache-control', 'no-store');
  response.end(JSON.stringify(body));
}
