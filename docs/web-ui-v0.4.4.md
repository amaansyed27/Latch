# Latch V0.4.4 web application

The independent Latch interface uses React, Vite, TypeScript, Tailwind CSS, Lucide icons, and Radix dialogs/tooltips. Inter is the UI font; JetBrains Mono is limited to technical values. Cool neutral surfaces and a restrained blue accent replace the previous brand direction.

## Structure and integration

- `router/web/src/pages`: landing, authentication, overview/devices, and product documentation.
- `components`: accessible controls, feedback, themes, and guided pairing.
- `hooks`: session and device state.
- `layouts`: public navigation and authenticated sidebar/mobile drawer.
- `lib/api.ts`: typed API client; credentials stay in same-origin cookies.
- `styles/app.css`: semantic light/dark tokens, tactile surface states, focus, responsive layouts, and reduced motion.

The existing Router resolves sessions and injects escaped, non-secret bootstrap data into Vite's HTML. OAuth validation, CSRF, code issuance, pairing, relay, and MCP remain server responsibilities. The consent page posts the existing OAuth form contract. Vercel serves built assets; local Router tests serve the same build from `router/public`.

## Development and checks

From `router`:

```sh
npm install
npm run build
npm run typecheck
npm run lint
npm test
npx playwright install chromium
npm run test:web
```

For local frontend work, `npm run dev:web` proxies API requests to port 8787. `node scripts/web-test-server.mjs` starts a local-only managed-auth fixture and real Router with in-memory coordination. It must never be deployed as an authentication service. Browser tests cover sessions, protected redirects, onboarding, online-only pairing success, revoke confirmation, errors, OAuth consent, responsive navigation, and persisted themes. Real relay and MCP E2E remain separate gates.

## Honest product state

Only actual device presence is called Online. Pairing completes only after a new device is online. Platform, last-seen, and ChatGPT connection status are omitted when unavailable. The ChatGPT guide follows the current [OpenAI connection documentation](https://developers.openai.com/plugins/deploy/connect-chatgpt): Settings → Security and login → Developer mode, then Plugins → plus. Availability depends on account/workspace policy.

The Windows installer stays at v0.4.2-beta.1. No MCP capabilities or telemetry were added. Files remain workspace-confined; commands run with the OS user's permissions and are not sandboxed.

Per the user's final instructions, visual sign-off and personal account/laptop/ChatGPT acceptance are deferred to the user. Automated browser checks are not a claim of personal ChatGPT acceptance. No public OpenAI submission was performed.
