import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';

const root = dirname(fileURLToPath(import.meta.url));
const port = Number.parseInt(process.env.PORT ?? process.argv[2] ?? '4177', 10);
const mime = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
]);

export const server = createServer(async (request, response) => {
  if (request.url === '/api/ping' && request.method === 'POST') {
    response.writeHead(200, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
    response.end(JSON.stringify({ status: 'clicked', request_id: randomUUID() }));
    return;
  }

  const pathname = request.url === '/' ? '/index.html' : request.url;
  if (!pathname || pathname.includes('..') || !['/index.html', '/app.js'].includes(pathname)) {
    response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    response.end('not found');
    return;
  }
  try {
    const body = await readFile(join(root, pathname.slice(1)));
    response.writeHead(200, {
      'content-type': mime.get(extname(pathname)) ?? 'application/octet-stream',
      'cache-control': 'no-store',
    });
    response.end(body);
  } catch {
    response.writeHead(500, { 'content-type': 'text/plain; charset=utf-8' });
    response.end('fixture read failed');
  }
});

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  server.listen(port, '127.0.0.1', () => {
    console.log(`Latch V0.6 fixture listening on http://127.0.0.1:${port}`);
  });
}
