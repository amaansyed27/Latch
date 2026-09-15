const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);
let localSnapshot = null;
let currentStatus = null;

function titleState(value) {
  const labels = {
    connected: "Connected",
    reconnecting: "Reconnecting",
    connecting: "Connecting",
    starting: "Starting",
    stopped: "Stopped",
    unpaired: "Not paired",
    paused: "Paused",
  };
  return labels[value] || "Checking";
}

function renderStatus(status) {
  currentStatus = status;
  $("version").textContent = `v${status.version}`;
  $("state-label").textContent = titleState(status.state);
  $("state-pill").className = `state-pill ${status.state}`;
  $("setup-view").hidden = status.paired;
  $("status-view").hidden = !status.paired;
  if (!status.paired) return;

  $("device-name").textContent = status.device_name || "This computer";
  $("detail-status").textContent = titleState(status.state);
  $("device-id").textContent = status.device_id || "—";
  $("router").textContent = status.router || "latch-router.vercel.app";
  $("remote-state").textContent = status.paused ? "Paused" : "Enabled";
  $("pause-remote").textContent = status.paused ? "Resume remote access" : "Pause remote access";

  const connected = status.state === "connected";
  document.querySelector(".connection-orb").classList.toggle("connected", connected);
  document.querySelector(".connection-orb").classList.toggle("paused", status.paused);
  $("connection-copy").textContent = status.paused
    ? "Remote requests are blocked locally. Pairing and the outbound connection are preserved."
    : connected
      ? "Ready for requests allowed by your local permissions."
      : ["reconnecting", "connecting", "starting"].includes(status.state)
        ? "Latch is reconnecting automatically."
        : "Latch is not connected right now.";
}

function makeButton(label, className, handler) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.textContent = label;
  button.addEventListener("click", handler);
  return button;
}

function renderRoots(config) {
  const list = $("roots-list");
  list.replaceChildren();
  $("roots-empty").hidden = config.roots.length > 0;
  for (const root of config.roots) {
    const row = document.createElement("div");
    row.className = "list-row surface";
    const copy = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = root.display_name;
    const path = document.createElement("small");
    path.textContent = root.canonical_path;
    copy.append(title, path);
    row.append(copy, makeButton("Remove", "text-button danger", async () => {
      if (!confirm(`Remove ${root.display_name} from approved folders?`)) return;
      renderLocal(await invoke("remove_folder", { rootId: root.root_id }));
    }));
    list.append(row);
  }
}

function serverInput(server, enabled = server.enabled) {
  const transport = server.transport || {};
  return {
    server_id: server.server_id,
    display_name: server.display_name,
    transport: transport.transport || "stdio",
    command: transport.command || "",
    arguments: transport.arguments || [],
    environment_references: transport.environment_references || {},
    url: transport.url || "",
    enabled,
    allow_remote: server.allow_remote,
  };
}

function editMcp(server) {
  const transport = server.transport || {};
  $("mcp-form").hidden = false;
  $("mcp-form-title").textContent = "Edit MCP";
  $("mcp-id").value = server.server_id;
  $("mcp-name").value = server.display_name;
  $("mcp-transport").value = transport.transport || "stdio";
  $("mcp-command").value = transport.command || "";
  $("mcp-args").value = (transport.arguments || []).join("\n");
  $("mcp-env").value = Object.entries(transport.environment_references || {}).map(([target, source]) => `${target}=${source}`).join("\n");
  $("mcp-url").value = transport.url || "";
  $("mcp-enabled").checked = server.enabled;
  $("mcp-remote").checked = server.allow_remote;
  toggleMcpTransport();
  $("mcp-name").focus();
}

function renderMcps(config) {
  const list = $("mcp-list");
  list.replaceChildren();
  $("mcp-empty").hidden = config.mcp_servers.length > 0;
  for (const server of config.mcp_servers) {
    const row = document.createElement("div");
    row.className = "list-row mcp-row surface";
    const copy = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = server.display_name;
    const transport = server.transport || {};
    const state = document.createElement("small");
    const remote = server.allow_remote ? "ChatGPT allowed" : "Local only";
    state.textContent = `${server.enabled ? "Enabled" : "Disabled"} · ${transport.transport === "http" ? "HTTP" : "Command / stdio"} · ${remote}`;
    copy.append(title, state);
    const actions = document.createElement("div");
    actions.className = "row-actions";
    actions.append(
      makeButton("Test", "text-button", async () => {
        try { alert(await invoke("test_mcp", { serverId: server.server_id })); }
        catch (error) { alert(String(error)); }
      }),
      makeButton(server.enabled ? "Disable" : "Enable", "text-button", async () => {
        renderLocal(await invoke("save_mcp", { input: serverInput(server, !server.enabled) }));
      }),
      makeButton("Edit", "text-button", () => editMcp(server)),
      makeButton("Remove", "text-button danger", async () => {
        if (!confirm(`Remove ${server.display_name}?`)) return;
        renderLocal(await invoke("remove_mcp", { serverId: server.server_id }));
      }),
    );
    row.append(copy, actions);
    list.append(row);
  }
}

function renderPermissions(config) {
  for (const input of document.querySelectorAll("[data-permission]")) {
    input.checked = Boolean(config.permissions[input.dataset.permission]);
  }
}

function renderActivity(activity) {
  const list = $("activity-list");
  list.replaceChildren();
  $("activity-empty").hidden = activity.length > 0;
  for (const entry of [...activity].reverse()) {
    const row = document.createElement("div");
    row.className = "activity-row";
    const time = document.createElement("time");
    time.textContent = new Date(entry.at_ms).toLocaleString();
    const copy = document.createElement("div");
    const action = document.createElement("strong");
    action.textContent = entry.action;
    const detail = document.createElement("small");
    detail.textContent = entry.detail;
    copy.append(action, detail);
    row.append(time, copy);
    list.append(row);
  }
}

function renderLocal(snapshot) {
  localSnapshot = snapshot;
  renderRoots(snapshot.config);
  renderMcps(snapshot.config);
  renderPermissions(snapshot.config);
  renderActivity(snapshot.activity);
  if (currentStatus) {
    currentStatus.paused = snapshot.config.paused;
    currentStatus.state = snapshot.config.paused ? "paused" : currentStatus.state === "paused" ? "connected" : currentStatus.state;
    renderStatus(currentStatus);
  }
}

async function refresh() {
  try {
    const status = await invoke("status");
    renderStatus(status);
    if (status.paired) renderLocal(await invoke("local_state"));
  } catch {
    $("state-label").textContent = "Unavailable";
  }
}

function switchTab(name) {
  for (const button of document.querySelectorAll("[data-tab]")) button.classList.toggle("active", button.dataset.tab === name);
  for (const panel of document.querySelectorAll("[data-panel]")) panel.classList.toggle("active", panel.dataset.panel === name);
}

function resetMcpForm() {
  $("mcp-form").reset();
  $("mcp-form").hidden = true;
  $("mcp-id").value = "";
  $("mcp-enabled").checked = true;
  $("mcp-remote").checked = false;
  $("mcp-message").textContent = "";
  toggleMcpTransport();
}

function toggleMcpTransport() {
  const http = $("mcp-transport").value === "http";
  $("mcp-http-fields").hidden = !http;
  $("mcp-stdio-fields").hidden = http;
}

function parseEnvReferences(value) {
  const result = {};
  for (const raw of value.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const index = line.indexOf("=");
    if (index <= 0 || index === line.length - 1) throw new Error("Environment references must use TARGET=LOCAL_ENV_NAME.");
    result[line.slice(0, index).trim()] = line.slice(index + 1).trim();
  }
  return result;
}

$("hide").addEventListener("click", () => invoke("hide_window"));
$("open-devices").addEventListener("click", () => invoke("open_devices"));
$("dashboard").addEventListener("click", () => invoke("open_dashboard"));
$("logs").addEventListener("click", () => invoke("open_logs"));
$("quit").addEventListener("click", () => invoke("quit_latch"));

for (const button of document.querySelectorAll("[data-tab]")) button.addEventListener("click", () => switchTab(button.dataset.tab));

$("pair").addEventListener("click", async () => {
  const button = $("pair");
  const message = $("pair-message");
  const code = $("pair-code").value.trim();
  if (!code) {
    message.textContent = "Paste the pairing code shown in your browser.";
    message.className = "form-message error";
    return;
  }
  button.disabled = true;
  button.textContent = "Pairing…";
  try {
    renderStatus(await invoke("pair", { code }));
    renderLocal(await invoke("local_state"));
    message.textContent = "Paired. Approve a folder and review local permissions next.";
    message.className = "form-message success";
    setTimeout(refresh, 1000);
  } catch (error) {
    message.textContent = String(error);
    message.className = "form-message error";
  } finally {
    button.disabled = false;
    button.textContent = "Pair this computer";
  }
});

$("restart").addEventListener("click", async () => {
  const button = $("restart");
  button.disabled = true;
  try { renderStatus(await invoke("restart")); setTimeout(refresh, 1000); }
  finally { button.disabled = false; }
});

$("pause-remote").addEventListener("click", async () => {
  const paused = !(localSnapshot?.config?.paused ?? false);
  renderLocal(await invoke("set_paused", { paused }));
  renderStatus(await invoke("status"));
});

$("add-folder").addEventListener("click", async () => renderLocal(await invoke("add_folder")));

for (const input of document.querySelectorAll("[data-permission]")) {
  input.addEventListener("change", async () => {
    try { renderLocal(await invoke("set_permission", { permission: input.dataset.permission, enabled: input.checked })); }
    catch (error) { input.checked = !input.checked; alert(String(error)); }
  });
}

$("new-mcp").addEventListener("click", () => {
  resetMcpForm();
  $("mcp-form").hidden = false;
  $("mcp-form-title").textContent = "Add MCP";
  $("mcp-name").focus();
});
$("cancel-mcp").addEventListener("click", resetMcpForm);
$("mcp-transport").addEventListener("change", toggleMcpTransport);
$("mcp-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const message = $("mcp-message");
  try {
    const input = {
      server_id: $("mcp-id").value || null,
      display_name: $("mcp-name").value.trim(),
      transport: $("mcp-transport").value,
      command: $("mcp-command").value.trim(),
      arguments: $("mcp-args").value.split(/\r?\n/).map((value) => value.trim()).filter(Boolean),
      environment_references: parseEnvReferences($("mcp-env").value),
      url: $("mcp-url").value.trim(),
      enabled: $("mcp-enabled").checked,
      allow_remote: $("mcp-remote").checked,
    };
    renderLocal(await invoke("save_mcp", { input }));
    resetMcpForm();
  } catch (error) {
    message.textContent = String(error);
    message.className = "form-message error";
  }
});

$("clear-activity").addEventListener("click", async () => {
  if (confirm("Clear local Latch activity history?")) renderLocal(await invoke("clear_activity"));
});

$("run-diagnostics").addEventListener("click", async () => {
  $("diagnostics-output").textContent = "Running diagnostics…";
  try { $("diagnostics-output").textContent = await invoke("run_diagnostics"); }
  catch (error) { $("diagnostics-output").textContent = String(error); }
});

refresh();
setInterval(async () => {
  try { renderStatus(await invoke("status")); }
  catch { /* leave the last known state visible */ }
}, 2500);
