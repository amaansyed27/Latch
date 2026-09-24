import { createInterface } from 'node:readline';
import { chromium } from 'playwright';

const contexts = new Map();
const tabs = new Map();
const elements = new Map();
let elementSequence = 1;
let eventSequence = 1;
const MAX_EVENTS = 500;
const MAX_SNAPSHOT = 64 * 1024;
const headless = process.env.LATCH_BROWSER_HEADLESS === '1';

function bounded(value, max = MAX_SNAPSHOT) {
  const text = String(value ?? '');
  return text.length <= max ? text : text.slice(0, max);
}

function pushEvent(record, bucket, kind, text) {
  record[bucket].push({ sequence: eventSequence++, kind, text: bounded(text, 4096) });
  if (record[bucket].length > MAX_EVENTS) record[bucket].splice(0, record[bucket].length - MAX_EVENTS);
}

function eventsSince(record, afterSequence) {
  return {
    console: record.console.filter((entry) => entry.sequence > afterSequence),
    network: record.network.filter((entry) => entry.sequence > afterSequence),
    downloads: record.downloads.filter((entry) => entry.sequence > afterSequence),
  };
}

function attachPage(tabId, page) {
  const record = tabs.get(tabId);
  if (!record) return;
  page.on('console', (message) => pushEvent(record, 'console', message.type(), message.text()));
  page.on('pageerror', (error) => pushEvent(record, 'console', 'pageerror', error.message));
  page.on('request', (request) => pushEvent(record, 'network', 'request', `${request.method()} ${request.url()}`));
  page.on('requestfailed', (request) => pushEvent(record, 'network', 'request_failed', `${request.method()} ${request.url()} · ${request.failure()?.errorText ?? 'failed'}`));
  page.on('response', (response) => {
    if (response.status() >= 400) pushEvent(record, 'network', 'response_error', `${response.status()} ${response.url()}`);
  });
  page.on('framenavigated', (frame) => {
    if (frame === page.mainFrame()) pushEvent(record, 'network', 'navigation', frame.url());
  });
  page.on('download', (download) => pushEvent(record, 'downloads', 'download', download.suggestedFilename()));
}

function tabRecord(tabId) {
  const record = tabs.get(tabId);
  if (!record) throw new Error(`tab_not_found:${tabId}`);
  return record;
}

function contextRecord(contextId) {
  const record = contexts.get(contextId);
  if (!record) throw new Error(`context_not_found:${contextId}`);
  return record;
}

async function tabInfo(tabId, record = tabRecord(tabId)) {
  return {
    tab_id: tabId,
    context_id: record.contextId,
    url: bounded(record.page.url(), 8192),
    title: bounded(await record.page.title(), 1024),
  };
}

function locatorFor(page, target) {
  if (target.element_ref) {
    const stored = elements.get(target.element_ref);
    if (!stored || stored.page !== page) throw new Error('stale_browser_ref');
    return stored.locator;
  }
  if (target.role) return page.getByRole(target.role, { name: target.name, exact: Boolean(target.exact) });
  if (target.text) return page.getByText(target.text, { exact: Boolean(target.exact) });
  if (target.label) return page.getByLabel(target.label, { exact: Boolean(target.exact) });
  if (target.test_id) return page.getByTestId(target.test_id);
  if (target.css) return page.locator(target.css);
  throw new Error('invalid_browser_target');
}

async function verify(page, verification) {
  if (!verification) return null;
  if (verification.url_contains) {
    const matched = await page.waitForURL((url) => url.toString().includes(verification.url_contains), { timeout: 5000 }).then(() => true).catch(() => false);
    if (!matched) return false;
  }
  if (verification.text_present) {
    const visible = await page.getByText(verification.text_present, { exact: false }).first().waitFor({ state: 'visible', timeout: 5000 }).then(() => true).catch(() => false);
    if (!visible) return false;
  }
  if (verification.selector_visible) {
    const visible = await page.locator(verification.selector_visible).first().waitFor({ state: 'visible', timeout: 5000 }).then(() => true).catch(() => false);
    if (!visible) return false;
  }
  return true;
}

async function handle(operation, params) {
  switch (operation) {
    case 'status':
      return { provider: 'playwright', version: '1.63.0', contexts: contexts.size, tabs: tabs.size, headless };
    case 'context.create': {
      const contextId = params.context_id;
      if (contexts.has(contextId)) throw new Error('context_exists');
      let context;
      let browser = null;
      if (params.profile === 'persistent') {
        if (!params.profile_dir) throw new Error('persistent_profile_dir_required');
        context = await chromium.launchPersistentContext(params.profile_dir, { headless, acceptDownloads: true });
      } else {
        browser = await chromium.launch({ headless });
        context = await browser.newContext({ acceptDownloads: true });
      }
      contexts.set(contextId, { context, browser, profile: params.profile });
      return null;
    }
    case 'context.list':
      return [...contexts.entries()].map(([contextId, record]) => ({ context_id: contextId, profile: record.profile }));
    case 'context.close': {
      const record = contextRecord(params.context_id);
      for (const [tabId, tab] of tabs) {
        if (tab.contextId === params.context_id) {
          tabs.delete(tabId);
          for (const [ref, element] of elements) if (element.page === tab.page) elements.delete(ref);
        }
      }
      await record.context.close();
      if (record.browser) await record.browser.close().catch(() => {});
      contexts.delete(params.context_id);
      return null;
    }
    case 'tab.create': {
      const context = contextRecord(params.context_id).context;
      const page = await context.newPage();
      const tabId = params.tab_id;
      tabs.set(tabId, { contextId: params.context_id, page, console: [], network: [], downloads: [] });
      attachPage(tabId, page);
      const before = eventSequence - 1;
      if (params.url) await page.goto(params.url, { waitUntil: 'domcontentloaded', timeout: 30000 });
      return { ...(await tabInfo(tabId)), events: eventsSince(tabRecord(tabId), before) };
    }
    case 'tab.list': {
      const result = [];
      for (const [tabId, record] of tabs) {
        if (!params.context_id || record.contextId === params.context_id) result.push(await tabInfo(tabId, record));
      }
      return result;
    }
    case 'tab.close': {
      const record = tabRecord(params.tab_id);
      await record.page.close();
      tabs.delete(params.tab_id);
      for (const [ref, element] of elements) if (element.page === record.page) elements.delete(ref);
      return null;
    }
    case 'navigate': {
      const record = tabRecord(params.tab_id);
      const before = eventSequence - 1;
      await record.page.goto(params.url, { waitUntil: 'domcontentloaded', timeout: 30000 });
      return { ...(await tabInfo(params.tab_id, record)), events: eventsSince(record, before) };
    }
    case 'snapshot': {
      const record = tabRecord(params.tab_id);
      const locator = record.page.locator('body');
      const full = await locator.ariaSnapshot({ timeout: 10000 });
      return {
        url: bounded(record.page.url(), 8192),
        title: bounded(await record.page.title(), 1024),
        aria: bounded(full),
        truncated: full.length > MAX_SNAPSHOT,
      };
    }
    case 'find': {
      const record = tabRecord(params.tab_id);
      for (const [ref, element] of elements) if (element.page === record.page) elements.delete(ref);
      const locator = locatorFor(record.page, params.target);
      const count = Math.min(await locator.count(), params.max_results ?? 10);
      const result = [];
      for (let index = 0; index < count; index += 1) {
        const item = locator.nth(index);
        const ref = `be_${elementSequence++}`;
        elements.set(ref, { page: record.page, locator: item });
        const metadata = await item.evaluate((node) => ({
          tag: node.tagName?.toLowerCase?.() ?? '',
          role: node.getAttribute?.('role') ?? null,
          name: node.getAttribute?.('aria-label') ?? node.textContent ?? '',
        })).catch(() => ({ tag: '', role: null, name: '' }));
        result.push({
          element_ref: ref,
          role: metadata.role,
          name: bounded(metadata.name, 1024),
          tag: metadata.tag,
          visible: await item.isVisible().catch(() => false),
          enabled: await item.isEnabled().catch(() => false),
          bounds: await item.boundingBox().catch(() => null),
        });
      }
      return result;
    }
    case 'act': {
      const record = tabRecord(params.tab_id);
      const locator = locatorFor(record.page, params.target).first();
      const action = params.action;
      const before = eventSequence - 1;
      let providerVerification = null;
      switch (action.kind) {
        case 'click': await locator.click({ timeout: 10000 }); break;
        case 'fill':
          await locator.fill(action.value, { timeout: 10000 });
          providerVerification = (await locator.inputValue().catch(() => null)) === action.value;
          break;
        case 'press': await locator.press(action.key, { timeout: 10000 }); break;
        case 'check':
          await locator.check({ timeout: 10000 });
          providerVerification = await locator.isChecked().catch(() => false);
          break;
        case 'uncheck':
          await locator.uncheck({ timeout: 10000 });
          providerVerification = !(await locator.isChecked().catch(() => true));
          break;
        case 'select_option':
          await locator.selectOption(action.value, { timeout: 10000 });
          providerVerification = (await locator.inputValue().catch(() => null)) === action.value;
          break;
        case 'hover': await locator.hover({ timeout: 10000 }); break;
        case 'focus': await locator.focus({ timeout: 10000 }); break;
        default: throw new Error(`unsupported_browser_action:${action.kind}`);
      }
      const requestedVerification = await verify(record.page, params.verification);
      return {
        url: bounded(record.page.url(), 8192),
        title: bounded(await record.page.title(), 1024),
        verification: requestedVerification ?? providerVerification,
        events: eventsSince(record, before),
      };
    }
    case 'console': {
      const record = tabRecord(params.tab_id);
      return record.console.filter((entry) => entry.sequence > (params.after_sequence ?? 0)).slice(0, params.max_entries ?? 100);
    }
    case 'network': {
      const record = tabRecord(params.tab_id);
      return record.network.filter((entry) => entry.sequence > (params.after_sequence ?? 0)).slice(0, params.max_entries ?? 100);
    }
    case 'downloads': {
      const record = tabRecord(params.tab_id);
      return record.downloads.filter((entry) => entry.sequence > (params.after_sequence ?? 0)).slice(0, params.max_entries ?? 100);
    }
    case 'screenshot': {
      const record = tabRecord(params.tab_id);
      const data = await record.page.screenshot({ type: 'jpeg', quality: 75, fullPage: false });
      const viewport = record.page.viewportSize();
      return {
        mime_type: 'image/jpeg',
        data_base64: data.toString('base64'),
        width: viewport?.width ?? null,
        height: viewport?.height ?? null,
      };
    }
    case 'page.state':
      return tabInfo(params.tab_id);
    case 'shutdown': {
      for (const record of contexts.values()) {
        await record.context.close().catch(() => {});
        if (record.browser) await record.browser.close().catch(() => {});
      }
      contexts.clear();
      tabs.clear();
      elements.clear();
      return { shutting_down: true };
    }
    default:
      throw new Error(`unknown_browser_operation:${operation}`);
  }
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  if (!line.trim()) continue;
  let request;
  try {
    request = JSON.parse(line);
    const result = await handle(request.operation, request.params ?? {});
    process.stdout.write(`${JSON.stringify({ id: request.id, result, error: null })}\n`);
    if (request.operation === 'shutdown') break;
  } catch (error) {
    const id = request?.id ?? 0;
    const message = error instanceof Error ? error.message : String(error);
    process.stdout.write(`${JSON.stringify({ id, result: null, error: bounded(message, 4096) })}\n`);
  }
}
