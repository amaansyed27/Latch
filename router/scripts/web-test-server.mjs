// Local test server only: managed-auth fixture, real Router, and in-memory relay.
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { testConfig } from "../dist/src/config.js";
import { MemoryAuthorizationStore } from "../dist/src/authorization-store.js";
import { MemoryCoordinator } from "../dist/src/memory-coordinator.js";
import { createRouterRuntime } from "../dist/src/runtime.js";

const sessions = new Map();
const auth = createServer(async (req, res) => {
  res.setHeader("content-type", "application/json");
  if (req.url === "/get-session") {
    const key = /test-session=([^;]+)/.exec(req.headers.cookie || "")?.[1];
    res.end(
      JSON.stringify(sessions.has(key) ? { user: sessions.get(key) } : null),
    );
    return;
  }
  if (req.url === "/sign-out") {
    const key = /test-session=([^;]+)/.exec(req.headers.cookie || "")?.[1];
    sessions.delete(key);
    res.setHeader("set-cookie", "test-session=; Path=/; HttpOnly; Max-Age=0");
    res.end("{}");
    return;
  }
  if (["/sign-up/email", "/sign-in/email"].includes(req.url)) {
    let body = "";
    for await (const chunk of req) body += chunk;
    const { email, password } = JSON.parse(body);
    if (password === "wrong-password") {
      res.statusCode = 401;
      res.end("{}");
      return;
    }
    const key = randomUUID();
    sessions.set(key, { id: email, email });
    res.setHeader(
      "set-cookie",
      `test-session=${key}; Path=/; HttpOnly; SameSite=Lax`,
    );
    res.end("{}");
    return;
  }
  if (req.url === "/request-password-reset" || req.url === "/reset-password") {
    res.end("{}");
    return;
  }
  res.statusCode = 404;
  res.end("{}");
});
await new Promise((resolve) => auth.listen(0, "127.0.0.1", resolve));
const config = testConfig({
  publicBaseUrl: "http://127.0.0.1:8787",
  neonAuthBaseUrl: `http://127.0.0.1:${auth.address().port}`,
  allowLegacyAppToken: false,
});
const runtime = createRouterRuntime(
  config,
  new MemoryCoordinator(),
  new MemoryAuthorizationStore(),
);
await runtime.ready;
runtime.server.listen(8787, "127.0.0.1", () =>
  console.log("Web test server ready at http://127.0.0.1:8787"),
);
