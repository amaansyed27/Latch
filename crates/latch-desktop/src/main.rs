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
        fs,
        path::{Path, PathBuf},
        process::{Command, Output},
        time::Duration,
    };

    use latch_link::{
        default_device_id_path, load_device_credential, load_or_create_device_id, LinkClient,
        LinkConfig,
    };
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
        updated_at: Option<u64>,
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
                open_devices,
                open_dashboard,
                open_logs,
                hide_window,
                quit_latch
            ])
            .setup(move |app| {
                let open = MenuItemBuilder::with_id("open", "Open Latch").build(app)?;
                let devices =
                    MenuItemBuilder::with_id("devices", "Manage devices").build(app)?;
                let restart =
                    MenuItemBuilder::with_id("restart", "Restart connection").build(app)?;
                let quit = MenuItemBuilder::with_id("quit", "Quit Latch").build(app)?;
                let menu = MenuBuilder::new(app)
                    .items(&[&open, &devices, &restart, &quit])
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
                        "devices" => {
                            let _ = open_url(DEVICES_URL);
                        }
                        "restart" => {
                            let _ = run_cli(&["restart"]);
                        }
                        "quit" => {
                            let _ = run_cli(&["stop"]);
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
                    let _ = run_cli(&["start", "--startup"]);
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
        load_device_credential(device_id)
            .ok()
            .flatten()
            .is_some()
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
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

        if let Some(status) = file.as_mut() {
            if status.state != "unpaired" && !process_exists(status.pid) {
                status.state = "stopped".to_owned();
            }
        }

        let fallback_name = config
            .as_ref()
            .map_or_else(|| "This computer".to_owned(), |value| value.device_name().to_owned());
        let fallback_router = config
            .as_ref()
            .and_then(|value| value.router_url().host_str())
            .unwrap_or("latch-router.vercel.app")
            .to_owned();

        DesktopStatus {
            version: file
                .as_ref()
                .map_or_else(|| env!("CARGO_PKG_VERSION").to_owned(), |value| value.version.clone()),
            device_name: file
                .as_ref()
                .map_or(fallback_name, |value| value.device_name.clone()),
            device_id: file.as_ref().map(|value| value.device_id.clone()),
            state: if paired {
                file.as_ref()
                    .map_or_else(|| "starting".to_owned(), |value| value.state.clone())
            } else {
                "unpaired".to_owned()
            },
            router: file
                .as_ref()
                .map_or(fallback_router, |value| value.router.clone()),
            paired,
            updated_at: file.as_ref().map(|value| value.updated_at),
        }
    }

    #[tauri::command]
    fn status() -> DesktopStatus {
        status_snapshot()
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

        let _ = run_cli(&["stop"]);
        run_cli(&["start", "--startup"])?;
        tauri::async_runtime::sleep(Duration::from_millis(900)).await;
        Ok(status_snapshot())
    }

    #[tauri::command]
    async fn restart() -> Result<DesktopStatus, String> {
        run_cli(&["restart"])?;
        tauri::async_runtime::sleep(Duration::from_millis(700)).await;
        Ok(status_snapshot())
    }

    #[tauri::command]
    fn run_diagnostics() -> Result<String, String> {
        let output = run_cli(&["doctor"])?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
        let logs = local_app_dir().join("logs");
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        Command::new("explorer.exe")
            .arg(logs)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[tauri::command]
    fn hide_window(app: AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }

    #[tauri::command]
    fn quit_latch(app: AppHandle) {
        let _ = run_cli(&["stop"]);
        app.exit(0);
    }

    fn open_url(url: &str) -> Result<(), String> {
        Command::new("explorer.exe")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
