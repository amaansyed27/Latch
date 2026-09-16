use std::error::Error;

use latch_link::{LinkClient, LinkConfig};
use tracing_subscriber::EnvFilter;

mod lifecycle;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "TLS crypto provider is already configured differently")?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let background = args.as_slice() == ["run", "--background"];
    init_logging(background)?;

    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("latch {}", env!("CARGO_PKG_VERSION"));
        }
        [flag] if flag == "--help" || flag == "-h" => print_help(),
        [command, code] if command == "pair" => {
            let config = LinkConfig::from_env()?;
            println!(
                "Latch\n\nPairing this computer...\nDevice: {}",
                config.device_name()
            );
            let identity = LinkClient::pair(&config, code).await?;
            println!(
                "\n✓ Paired successfully\n✓ Credential stored securely\n✓ Latch is ready\nDevice ID: {}",
                identity.device_id
            );
            lifecycle::stop(true)?;
            start_product(true)?;
        }
        [] => lifecycle::run(false).await?,
        [command] if command == "run" => lifecycle::run(false).await?,
        [command, flag] if command == "run" && flag == "--background" => {
            lifecycle::run(true).await?;
        }
        [command] if command == "start" => start_product(false)?,
        // Compatibility with the V0.5 desktop app. New desktop builds use worker-start directly.
        [command, flag] if command == "start" && flag == "--startup" => {
            lifecycle::start(true)?;
        }
        [command] if command == "stop" => stop_product(false)?,
        [command] if command == "restart" => restart_product()?,
        [command] if command == "status" => lifecycle::print_status()?,
        [command] if command == "doctor" => lifecycle::doctor().await?,
        [command] if command == "reset" => lifecycle::reset()?,
        [command] if command == "supervise" => lifecycle::supervise()?,
        [command] if command == "worker-start" => lifecycle::start(true)?,
        [command] if command == "worker-stop" => lifecycle::stop(true)?,
        [command] if command == "worker-restart" => {
            lifecycle::stop(true)?;
            lifecycle::start(true)?;
        }
        _ => {
            print_help();
            return Err("invalid command".into());
        }
    }
    Ok(())
}

fn start_product(quiet: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
    #[cfg(windows)]
    {
        if launch_desktop().is_ok() {
            if !quiet {
                println!("Latch opened. The tray app will keep the local connection running.");
            }
            return Ok(());
        }
    }

    lifecycle::start(quiet)?;
    Ok(())
}

fn stop_product(quiet: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
    lifecycle::stop(true)?;
    #[cfg(windows)]
    stop_desktop();
    if !quiet {
        println!("Latch stopped.");
    }
    Ok(())
}

fn restart_product() -> Result<(), Box<dyn Error + Send + Sync>> {
    lifecycle::stop(true)?;
    lifecycle::start(true)?;
    #[cfg(windows)]
    {
        let _ = launch_desktop();
    }
    println!("Latch restarted.");
    Ok(())
}

#[cfg(windows)]
fn launch_desktop() -> std::io::Result<()> {
    use std::{
        io::{Error, ErrorKind},
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let current = std::env::current_exe()?;
    let desktop = current
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "Latch installation folder is missing"))?
        .join("LatchDesktop.exe");
    if !desktop.exists() {
        return Err(Error::new(
            ErrorKind::NotFound,
            "LatchDesktop.exe is not installed",
        ));
    }
    Command::new(desktop)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

#[cfg(windows)]
fn stop_desktop() {
    use std::{os::windows::process::CommandExt, process::Command};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("taskkill")
        .args(["/IM", "LatchDesktop.exe", "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

fn init_logging(background: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if background {
        let file = lifecycle::init_file_logging()?;
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(filter)
            .with_target(false)
            .with_ansi(false)
            .with_writer(move || file.try_clone().expect("log file remains available"))
            .try_init()?;
    } else if std::env::var("LATCH_LOG").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(false)
            .try_init()?;
    } else {
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(filter)
            .with_target(false)
            .try_init()?;
    }
    Ok(())
}

fn print_help() {
    println!("Latch {}\n\nUsage: latch <command>\n\nCommands:\n  pair <code>  Pair this computer and open Latch\n  start        Open the Latch tray app and ensure the connection is running\n  stop         Stop Latch and close the tray app\n  restart      Restart the local connection and open Latch\n  status       Show connection status\n  doctor       Check installation and connectivity\n  reset        Remove local identity and credential\n  run          Run the worker in the foreground (developer use)\n  --version    Print version\n  --help       Print help", env!("CARGO_PKG_VERSION"));
}
