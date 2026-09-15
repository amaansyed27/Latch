import { existsSync, readFileSync } from "node:fs";
import { resolve, extname } from "node:path";
import type { ServerResponse } from "node:http";

export interface WebIdentity {
  id: string;
  email?: string;
}
const publicRoot = resolve(
  process.cwd(),
  existsSync("public/index.html") ? "public" : "router/public",
);
const template = (): string =>
  readFileSync(resolve(publicRoot, "index.html"), "utf8");
export function safeReturnTo(value: string | null): string {
  return value &&
    value.startsWith("/") &&
    !value.startsWith("//") &&
    !value.includes("\\")
    ? value
    : "/dashboard";
}
export function renderPage(
  response: ServerResponse,
  _path: string,
  authReady: boolean,
  returnTo: string | null,
  identity: WebIdentity | null,
): void {
  send(response, 200, {
    identity,
    authReady,
    returnTo: safeReturnTo(returnTo),
  });
}
export function renderAuthorization(
  response: ServerResponse,
  params: Record<string, string>,
  scopes: string[],
  csrf: string,
  identity: WebIdentity,
): void {
  send(response, 200, {
    identity,
    authReady: true,
    returnTo: "/dashboard",
    authorization: { params, scopes, csrf },
  });
}
export function renderError(
  response: ServerResponse,
  status: number,
  title: string,
  message: string,
  identity: WebIdentity | null = null,
): void {
  send(response, status, {
    identity,
    authReady: true,
    returnTo: "/dashboard",
    error: { title, message },
  });
}
function send(response: ServerResponse, status: number, data: unknown): void {
  const serialized = JSON.stringify(data)
    .replace(/</g, "\\u003c")
    .replace(/>/g, "\\u003e")
    .replace(/&/g, "\\u0026");
  response.statusCode = status;
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(
    template().replace(
      "<!--latch-bootstrap-->",
      `<script id="latch-bootstrap" type="application/json">${serialized}</script>`,
    ),
  );
}
export function serveAsset(response: ServerResponse, path: string): boolean {
  if (!/^\/(?:assets\/[a-zA-Z0-9._-]+|favicon\.svg|theme\.js)$/.test(path))
    return false;
  const file = resolve(publicRoot, `.${path}`);
  if (!existsSync(file)) return false;
  const type: Record<string, string> = {
    ".css": "text/css",
    ".js": "text/javascript",
    ".svg": "image/svg+xml",
    ".woff2": "font/woff2",
  };
  response.setHeader(
    "content-type",
    type[extname(file)] || "application/octet-stream",
  );
  response.setHeader(
    "cache-control",
    path.startsWith("/assets/")
      ? "public, max-age=31536000, immutable"
      : "public, max-age=300",
  );
  response.end(readFileSync(file));
  return true;
}
