#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("Latch Desktop is currently available on Windows only.");
}

#[cfg(windows)]
fn main() {
    windows_app::run();
}

#[cfg(windows)]
mod windows_app {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        process::{Command, Output},
        time::Duration,
    };

    use latch_core::{ApprovalId, McpServerId, RootId};
    use latch_link::{
        default_device_id_path, load_device_credential, load_or_create_device_id, LinkClient,
        LinkConfig,
    };
    use latch_local::{
        ActivityEntry, ApprovalDecision, ApprovalRequest, Capability, LocalConfig, LocalStore,
        McpServerConfig, McpTransportConfig, PermissionMode, PermissionPreset,
    };
    use latch_mcp_client::test_connection;
    use rfd::FileDialog;
    use serde::{Deserialize, Serialize};
    use tauri::{
        menu::{MenuBuilder, MenuItemBuilder},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
        AppHandle, Manager, WindowEvent,
    };

    const DEVICES_URL: &str = "https://latch-router.vercel.app/devices";
    const DASHBOARD_URL: &str = "https://latch-router.vercel.app/dashboard";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(Debug, Clone, Deserialize)]
    struct StatusFile {
        version: String,
        device_name: String,
        device_id: String,
        state: String,
        router: String,
        pid: u32,
        updated_at: u64,
    }

    #[derive(Debug, Clone, Serialize)]
    struct DesktopStatus {
        version: String,
        device_name: String,
        device_id: Option<String>,
        state: String,
        router: String,
        paired: bool,
        paused: bool,
        updated_at: Option<u64>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct DesktopLocalState {
        config: LocalConfig,
        activity: Vec<ActivityEntry>,
        approvals: Vec<ApprovalRequest>,
    }

    #[derive(Debug, Deserialize)]
    struct McpInput {
        server_id: Option<String>,
        display_name: String,
        transport: String,
        #[serde(default)]
        command: String,
        #[serde(default)]
        arguments: Vec<String>,
        #[serde(default)]
        environment_references: BTreeMap<String, String>,
        #[serde(default)]
        url: String,
        enabled: bool,
        allow_remote: bool,
    }

    pub fn run() {
        let startup = std::env::args().any(|arg| arg == "--startup");

        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                show_window(app);
            }))
            .invoke_handler(tauri::generate_handler![
                status,
                pair,
                restart,
                run_diagnostics,
                local_state,
                add_folder,
                remove_folder,
                set_capability_permission,
                set_permission_preset,
                resolve_approval,
                set_paused,
                save_mcp,
                remove_mcp,
                test_mcp,
                clear_activity,
                open_devices,
                open_dashboard,
                open_logs,
                hide_window,
                quit_latch
            ])
            .setup(move |app| {
                let open = MenuItemBuilder::with_id("open", "Open Latch").build(app)?;
                let connection = MenuItemBuilder::with_id("connection", "Connection status")
                    .enabled(false)
                    .build(app)?;
                let pause =
                    MenuItemBuilder::with_id("pause", "Pause / resume remote access").build(app)?;
                let restart =
                    MenuItemBuilder::with_id("restart", "Restart connection").build(app)?;
                let quit = MenuItemBuilder::with_id("quit", "Quit Latch").build(app)?;
                let menu = MenuBuilder::new(app)
                    .items(&[&open, &connection, &pause, &restart, &quit])
                    .build()?;

                TrayIconBuilder::with_id("latch")
                    .icon(
                        app.default_window_icon()
                            .expect("Latch desktop icon should be bundled")
                            .clone(),
                    )
                    .tooltip("Latch — your computer, available to ChatGPT")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "open" => show_window(app),
                        "pause" => {
                            if let Ok(store) = local_store() {
                                if let Ok(config) = store.load() {
                                    let _ = store.set_paused(!config.paused);
                                }
                            }
                            show_window(app);
                        }
                        "restart" => {
                            let _ = run_cli(&["worker-restart"]);
                        }
                        "quit" => {
                            let _ = run_cli(&["worker-stop"]);
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_window(tray.app_handle());
                        }
                    })
                    .build(app)?;

                if is_paired() {
                    let _ = run_cli(&["worker-start"]);
                }
                if !startup || !is_paired() {
                    show_window(app.handle());
                }
                Ok(())
            })
            .on_window_event(|window, event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            })
            .run(tauri::generate_context!())
            .expect("failed to run Latch Desktop");
    }

    fn show_window(app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.center();
            let _ = window.set_focus();
        }
    }

    fn local_app_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Latch")
    }

    fn local_store() -> Result<LocalStore, String> {
        LocalStore::default_location().map_err(|error| error.to_string())
    }

    fn cli_path() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let dir = exe
            .parent()
            .ok_or_else(|| "Latch installation folder is unavailable.".to_owned())?;
        for candidate in ["latch.exe", "latch-link.exe"] {
            let path = dir.join(candidate);
            if path.exists() {
                return Ok(path);
            }
        }
        Err("Latch command component is missing. Reinstall Latch.".to_owned())
    }

    fn run_cli(args: &[&str]) -> Result<Output, String> {
        use std::os::windows::process::CommandExt;

        Command::new(cli_path()?)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| error.to_string())
            .and_then(|output| {
                if output.status.success() {
                    Ok(output)
                } else {
                    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    Err(if detail.is_empty() {
                        format!("Latch command failed with {}.", output.status)
                    } else {
                        detail
                    })
                }
            })
    }

    fn is_paired() -> bool {
        let Ok(path) = default_device_id_path() else {
            return false;
        };
        if !path.exists() {
            return false;
        }
        let Ok(device_id) = load_or_create_device_id(&path) else {
            return false;
        };
        load_device_credential(device_id).ok().flatten().is_some()
    }

    fn process_exists(pid: u32) -> bool {
        use std::os::windows::process::CommandExt;

        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|output| {
                let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                text.contains(&pid.to_string())
                    && (text.contains("latch.exe") || text.contains("latch-link.exe"))
            })
    }

    fn read_status_file(path: &Path) -> Option<StatusFile> {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    fn status_snapshot() -> DesktopStatus {
        let config = LinkConfig::from_env().ok();
        let mut file = read_status_file(&local_app_dir().join("status.json"));
        let paired = is_paired();
        let paused = local_store()
            .and_then(|store| store.load().map_err(|error| error.to_string()))
            .is_ok_and(|local| local.paused);
        if let Some(status) = file.as_mut() {
            if status.state != "unpaired" && !process_exists(status.pid) {
                status.state.clear();
                status.state.push_str("stopped");
            }
        }
        let fallback_name = config.as_ref().map_or_else(
            || "This computer".to_owned(),
            |value| value.device_name().to_owned(),
        );
        let fallback_router = config
            .as_ref()
            .and_then(|value| value.router_url().host_str())
            .unwrap_or("latch-router.vercel.app")
            .to_owned();
        DesktopStatus {
            version: file.as_ref().map_or_else(
                || env!("CARGO_PKG_VERSION").to_owned(),
                |value| value.version.clone(),
            ),
            device_name: file
                .as_ref()
                .map_or(fallback_name, |value| value.device_name.clone()),
            device_id: file.as_ref().map(|value| value.device_id.clone()),
            state: if !paired {
                "unpaired".to_owned()
            } else if paused {
                "paused".to_owned()
            } else {
                file.as_ref()
                    .map_or_else(|| "starting".to_owned(), |value| value.state.clone())
            },
            router: file
                .as_ref()
                .map_or(fallback_router, |value| value.router.clone()),
            paired,
            paused,
            updated_at: file.as_ref().map(|value| value.updated_at),
        }
    }

    fn local_snapshot() -> Result<DesktopLocalState, String> {
        let store = local_store()?;
        Ok(DesktopLocalState {
            config: store.load().map_err(|error| error.to_string())?,
            activity: store.activity().map_err(|error| error.to_string())?,
            approvals: store
                .pending_approvals()
                .map_err(|error| error.to_string())?,
        })
    }

    #[tauri::command]
    fn status() -> DesktopStatus {
        status_snapshot()
    }

    #[tauri::command]
    fn local_state() -> Result<DesktopLocalState, String> {
        local_snapshot()
    }

    #[tauri::command]
    async fn pair(code: String) -> Result<DesktopStatus, String> {
        let code = code.trim();
        if code.len() < 8 || code.len() > 512 {
            return Err("Enter the complete pairing code from Latch.".to_owned());
        }
        let config = LinkConfig::from_env().map_err(|error| error.to_string())?;
        LinkClient::pair(&config, code)
            .await
            .map_err(|error| error.to_string())?;
        let _ = run_cli(&["worker-stop"]);
        run_cli(&["worker-start"])?;
        tokio::time::sleep(Duration::from_millis(900)).await;
        Ok(status_snapshot())
    }

    #[tauri::command]
    async fn restart() -> Result<DesktopStatus, String> {
        run_cli(&["worker-restart"])?;
        tokio::time::sleep(Duration::from_millis(700)).await;
        Ok(status_snapshot())
    }

    #[tauri::command]
    fn add_folder() -> Result<DesktopLocalState, String> {
        let Some(path) = FileDialog::new()
            .set_title("Approve a folder for Latch")
            .pick_folder()
        else {
            return local_snapshot();
        };
        local_store()?
            .add_root(path)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn remove_folder(root_id: String) -> Result<DesktopLocalState, String> {
        let root_id = root_id
            .parse::<RootId>()
            .map_err(|_| "Invalid folder ID.".to_owned())?;
        local_store()?
            .remove_root(root_id)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn set_capability_permission(
        capability: String,
        mode: String,
    ) -> Result<DesktopLocalState, String> {
        local_store()?
            .set_permission_mode(
                parse_capability(&capability)?,
                parse_permission_mode(&mode)?,
            )
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn set_permission_preset(preset: String) -> Result<DesktopLocalState, String> {
        let preset = match preset.as_str() {
            "observe" => PermissionPreset::Observe,
            "work" => PermissionPreset::Work,
            "developer" => PermissionPreset::Developer,
            "full_control" => PermissionPreset::FullControl,
            _ => return Err("Unknown permission preset.".to_owned()),
        };
        local_store()?
            .set_permission_preset(preset)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn resolve_approval(
        approval_id: String,
        decision: String,
    ) -> Result<DesktopLocalState, String> {
        let approval_id = approval_id
            .parse::<ApprovalId>()
            .map_err(|_| "Invalid approval ID.".to_owned())?;
        let decision = match decision.as_str() {
            "deny" => ApprovalDecision::Deny,
            "allow_once" => ApprovalDecision::AllowOnce,
            "allow_session" => ApprovalDecision::AllowSession,
            _ => return Err("Unknown approval decision.".to_owned()),
        };
        local_store()?
            .resolve_approval(approval_id, decision)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn set_paused(paused: bool) -> Result<DesktopLocalState, String> {
        local_store()?
            .set_paused(paused)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn save_mcp(input: McpInput) -> Result<DesktopLocalState, String> {
        let server_id = match input.server_id.as_deref() {
            Some(value) if !value.trim().is_empty() => value
                .parse::<McpServerId>()
                .map_err(|_| "Invalid MCP server ID.".to_owned())?,
            _ => McpServerId::new(),
        };
        let transport = match input.transport.as_str() {
            "stdio" => McpTransportConfig::Stdio {
                command: input.command,
                arguments: input.arguments,
                environment_references: input.environment_references,
            },
            "http" => McpTransportConfig::Http { url: input.url },
            _ => return Err("MCP transport must be stdio or http.".to_owned()),
        };
        local_store()?
            .upsert_mcp_server(McpServerConfig {
                server_id,
                display_name: input.display_name,
                transport,
                enabled: input.enabled,
                allow_remote: input.allow_remote,
            })
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn remove_mcp(server_id: String) -> Result<DesktopLocalState, String> {
        let server_id = server_id
            .parse::<McpServerId>()
            .map_err(|_| "Invalid MCP server ID.".to_owned())?;
        local_store()?
            .remove_mcp_server(server_id)
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    async fn test_mcp(server_id: String) -> Result<String, String> {
        let server_id = server_id
            .parse::<McpServerId>()
            .map_err(|_| "Invalid MCP server ID.".to_owned())?;
        let store = local_store()?;
        let server = store
            .load()
            .map_err(|error| error.to_string())?
            .mcp_servers
            .into_iter()
            .find(|server| server.server_id == server_id)
            .ok_or_else(|| "Local MCP server not found.".to_owned())?;
        let count = tokio::task::spawn_blocking(move || test_connection(&server))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        Ok(format!("Connected. {count} tool(s) available."))
    }

    #[tauri::command]
    fn clear_activity() -> Result<DesktopLocalState, String> {
        local_store()?
            .clear_activity()
            .map_err(|error| error.to_string())?;
        local_snapshot()
    }

    #[tauri::command]
    fn run_diagnostics() -> Result<String, String> {
        let output = run_cli(&["doctor"])?;
        let doctor = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let local = local_snapshot()?;
        Ok(format!(
            "{doctor}\nApproved folders          {}\nLocal MCP integrations    {}\nPending approvals         {}\nRemote access             {}",
            local.config.roots.len(),
            local.config.mcp_servers.len(),
            local.approvals.len(),
            if local.config.paused { "PAUSED" } else { "enabled" }
        ))
    }

    #[tauri::command]
    fn open_devices() -> Result<(), String> {
        open_url(DEVICES_URL)
    }

    #[tauri::command]
    fn open_dashboard() -> Result<(), String> {
        open_url(DASHBOARD_URL)
    }

    #[tauri::command]
    fn open_logs() -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        let logs = local_app_dir().join("logs");
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        Command::new("explorer.exe")
            .arg(logs)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[allow(clippy::needless_pass_by_value)]
    #[tauri::command]
    fn hide_window(app: AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    #[tauri::command]
    fn quit_latch(app: AppHandle) {
        let _ = run_cli(&["worker-stop"]);
        app.exit(0);
    }

    fn parse_capability(value: &str) -> Result<Capability, String> {
        match value {
            "files_read" => Ok(Capability::FilesRead),
            "files_write" => Ok(Capability::FilesWrite),
            "exec" => Ok(Capability::Exec),
            "terminal" => Ok(Capability::Terminal),
            "application_control" => Ok(Capability::ApplicationControl),
            "ui_inspection" => Ok(Capability::UiInspection),
            "ui_control" => Ok(Capability::UiControl),
            "screen_capture" => Ok(Capability::ScreenCapture),
            "raw_input" => Ok(Capability::RawInput),
            "browser_isolated" => Ok(Capability::BrowserIsolated),
            "browser_authenticated" => Ok(Capability::BrowserAuthenticated),
            "clipboard_read" => Ok(Capability::ClipboardRead),
            "clipboard_write" => Ok(Capability::ClipboardWrite),
            "mcp_discovery" => Ok(Capability::McpDiscovery),
            "mcp_execution" => Ok(Capability::McpExecution),
            "native_system_control" => Ok(Capability::NativeSystemControl),
            _ => Err("Unknown capability.".to_owned()),
        }
    }

    fn parse_permission_mode(value: &str) -> Result<PermissionMode, String> {
        match value {
            "deny" => Ok(PermissionMode::Deny),
            "ask" => Ok(PermissionMode::Ask),
            "allow" => Ok(PermissionMode::Allow),
            _ => Err("Permission mode must be deny, ask, or allow.".to_owned()),
        }
    }

    fn open_url(url: &str) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        Command::new("explorer.exe")
            .arg(url)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
