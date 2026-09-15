const invoke = window.__TAURI__.core.invoke;

const $ = (id) => document.getElementById(id);

function titleState(value) {
  const labels = {
    connected: "Connected",
    reconnecting: "Reconnecting",
    connecting: "Connecting",
    starting: "Starting",
    stopped: "Stopped",
    unpaired: "Not paired",
  };
  return labels[value] || "Checking";
}

function render(status) {
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

  const connected = status.state === "connected";
  document.querySelector(".connection-orb").classList.toggle("connected", connected);
  $("connection-copy").textContent = connected
    ? "Ready for approved Latch requests."
    : ["reconnecting", "connecting", "starting"].includes(status.state)
      ? "Latch is reconnecting automatically."
      : "Latch is not connected right now.";
}

async function refresh() {
  try {
    render(await invoke("status"));
  } catch {
    $("state-label").textContent = "Unavailable";
  }
}

$("hide").addEventListener("click", () => invoke("hide_window"));
$("open-devices").addEventListener("click", () => invoke("open_devices"));
$("dashboard").addEventListener("click", () => invoke("open_dashboard"));
$("logs").addEventListener("click", () => invoke("open_logs"));

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
  message.textContent = "Creating a secure device credential…";
  message.className = "form-message";
  try {
    const status = await invoke("pair", { code });
    message.textContent = "Paired. Latch is starting in the tray.";
    message.className = "form-message success";
    render(status);
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
  try {
    render(await invoke("restart"));
    setTimeout(refresh, 1000);
  } finally {
    button.disabled = false;
  }
});

$("diagnostics").addEventListener("click", async () => {
  $("diagnostics-panel").hidden = false;
  $("diagnostics-output").textContent = "Running diagnostics…";
  try {
    $("diagnostics-output").textContent = await invoke("run_diagnostics");
  } catch (error) {
    $("diagnostics-output").textContent = String(error);
  }
});

$("close-diagnostics").addEventListener("click", () => {
  $("diagnostics-panel").hidden = true;
});

$("quit").addEventListener("click", () => invoke("quit_latch"));

refresh();
setInterval(refresh, 2500);
