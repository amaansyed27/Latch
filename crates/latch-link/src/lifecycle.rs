use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use latch_link::{
    default_device_id_path, delete_device_credential, load_device_credential,
    load_or_create_device_id, LinkClient, LinkConfig, LinkError,
};
use serde::{Deserialize, Serialize};

const APP_URL: &str = "https://latch-router.vercel.app/devices";

#[derive(Serialize, Deserialize)]
struct Status {
    version: String,
    device_name: String,
    device_id: String,
    state: String,
    router: String,
    pid: u32,
    updated_at: u64,
}

pub fn app_dir() -> Result<PathBuf, LinkError> {
    default_device_id_path()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or(LinkError::ApplicationDataUnavailable)
}

pub fn init_file_logging() -> io::Result<std::fs::File> {
    let logs = app_dir().map_err(io::Error::other)?.join("logs");
    fs::create_dir_all(&logs)?;
    let current = logs.join("current.log");
    if current
        .metadata()
        .is_ok_and(|metadata| metadata.len() > 1_048_576)
    {
        let previous = logs.join("previous.log");
        let _ = fs::remove_file(&previous);
        fs::rename(&current, previous)?;
    }
    OpenOptions::new().create(true).append(true).open(current)
}

pub async fn run(background: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = LinkConfig::from_env()?;
    let mut client = match LinkClient::new(config) {
        Ok(client) => client,
        Err(LinkError::DeviceNotPaired) => {
            write_simple_status("unpaired")?;
            if !background {
                println!(
                    "Latch is not paired.\n\nVisit:\n{APP_URL}\n\nThen run:\nlatch pair <code>"
                );
            }
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let identity = client.identity().clone();
    let router = LinkConfig::from_env()?
        .router_url()
        .host_str()
        .unwrap_or("unknown")
        .to_owned();
    write_status(
        &identity.device_name,
        &identity.device_id.to_string(),
        "connecting",
        &router,
    )?;
    if !background {
        println!(
            "Latch\nDevice: {}\nDevice ID: {}\nConnecting…",
            identity.device_name, identity.device_id
        );
    }
    client
        .run_with_status(|connected| {
            let _ = write_status(
                &identity.device_name,
                &identity.device_id.to_string(),
                if connected {
                    "connected"
                } else {
                    "reconnecting"
                },
                &router,
            );
        })
        .await?;
    Ok(())
}

pub fn start(quiet: bool) -> io::Result<()> {
    let dir = app_dir().map_err(io::Error::other)?;
    fs::create_dir_all(&dir)?;
    if running(&supervisor_pid_path(&dir)) {
        if !quiet {
            println!("Latch is already running.");
        }
        return Ok(());
    }
    let _ = fs::remove_file(dir.join("stopped"));
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("supervise")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_hidden(&mut command)?;
    if !quiet {
        println!("Latch started in the background.");
    }
    Ok(())
}

pub fn supervise() -> io::Result<()> {
    let dir = app_dir().map_err(io::Error::other)?;
    fs::create_dir_all(&dir)?;
    fs::write(supervisor_pid_path(&dir), std::process::id().to_string())?;
    let identity_path = default_device_id_path().map_err(io::Error::other)?;
    let paired = load_or_create_device_id(&identity_path)
        .ok()
        .and_then(|id| load_device_credential(id).ok().flatten())
        .is_some();
    if !paired {
        write_simple_status("unpaired").map_err(io::Error::other)?;
        let _ = fs::remove_file(supervisor_pid_path(&dir));
        return Ok(());
    }
    while !dir.join("stopped").exists() {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["run", "--background"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = spawn_hidden(&mut command)?;
        let _ = child.wait();
        if !dir.join("stopped").exists() {
            thread::sleep(Duration::from_secs(3));
        }
    }
    let _ = fs::remove_file(supervisor_pid_path(&dir));
    Ok(())
}

pub fn stop(quiet: bool) -> io::Result<()> {
    let dir = app_dir().map_err(io::Error::other)?;
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("stopped"), b"")?;
    for path in [dir.join("status.json"), supervisor_pid_path(&dir)] {
        if let Some(pid) = read_pid(&path).filter(|pid| process_exists(*pid)) {
            terminate(pid);
        }
    }
    write_simple_status("stopped").map_err(io::Error::other)?;
    if !quiet {
        println!("Latch stopped.");
    }
    Ok(())
}

pub fn print_status() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = app_dir()?.join("status.json");
    let mut status: Status = serde_json::from_slice(&fs::read(path)?)?;
    if !process_exists(status.pid) && status.state != "unpaired" {
        "stopped".clone_into(&mut status.state);
    }
    println!(
        "Latch {}\nDevice: {}\nDevice ID: {}\nStatus: {}\nRouter: {}",
        status.version,
        status.device_name,
        status.device_id,
        title(&status.state),
        status.router
    );
    Ok(())
}

pub async fn doctor() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Latch doctor (credentials are never displayed)");
    println!("Version                  OK {}", env!("CARGO_PKG_VERSION"));
    let path = default_device_id_path()?;
    let id = load_or_create_device_id(&path)?;
    println!("Device identity          OK {id}");
    if load_device_credential(id)?.is_none() {
        return Err("device is not paired".into());
    }
    println!("Credential Manager       OK credential present");
    let health: serde_json::Value = reqwest::get("https://latch-router.vercel.app/api/health")
        .await?
        .json()
        .await?;
    if health.get("status").and_then(|v| v.as_str()) != Some("ok") {
        return Err("router is unhealthy".into());
    }
    println!("Router health            OK");
    let _ = print_status();
    Ok(())
}

pub fn reset() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    print!("Remove this computer's local Latch identity and credential? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Reset cancelled.");
        return Ok(());
    }
    stop(true)?;
    let path = default_device_id_path()?;
    if path.exists() {
        let id = load_or_create_device_id(&path)?;
        delete_device_credential(id)?;
        fs::remove_file(path)?;
    }
    for name in ["status.json", "supervisor.pid", "stopped"] {
        let _ = fs::remove_file(app_dir()?.join(name));
    }
    println!("Local Latch identity and credential removed. The cloud device was not revoked.");
    Ok(())
}

fn write_simple_status(state: &str) -> Result<(), LinkError> {
    let config = LinkConfig::from_env()?;
    let id = load_or_create_device_id(&default_device_id_path()?)?;
    write_status(
        config.device_name(),
        &id.to_string(),
        state,
        config.router_url().host_str().unwrap_or("unknown"),
    )
    .map_err(|source| LinkError::IdentityIo {
        path: app_dir().unwrap_or_default().join("status.json"),
        source,
    })
}

fn write_status(device_name: &str, device_id: &str, state: &str, router: &str) -> io::Result<()> {
    let path = app_dir().map_err(io::Error::other)?.join("status.json");
    fs::create_dir_all(path.parent().expect("status path has parent"))?;
    fs::write(
        path,
        serde_json::to_vec(&Status {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            device_name: device_name.to_owned(),
            device_id: device_id.to_owned(),
            state: state.to_owned(),
            router: router.to_owned(),
            pid: std::process::id(),
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })?,
    )
}

fn supervisor_pid_path(dir: &Path) -> PathBuf {
    dir.join("supervisor.pid")
}
fn read_pid(path: &Path) -> Option<u32> {
    if path.file_name().is_some_and(|name| name == "status.json") {
        return fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Status>(&bytes).ok())
            .map(|status| status.pid);
    }
    fs::read_to_string(path).ok()?.trim().parse().ok()
}
fn running(path: &Path) -> bool {
    read_pid(path).is_some_and(process_exists)
}
fn title(state: &str) -> String {
    let mut chars = state.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(windows)]
fn spawn_hidden(command: &mut Command) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000).spawn()
}
#[cfg(not(windows))]
fn spawn_hidden(command: &mut Command) -> io::Result<std::process::Child> {
    command.spawn()
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|output| {
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            (text.contains("latch.exe") || text.contains("latch-link.exe"))
                && text.contains(&pid.to_string())
        })
}
#[cfg(not(windows))]
fn process_exists(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

#[cfg(windows)]
fn terminate(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}
#[cfg(not(windows))]
fn terminate(_pid: u32) {}
