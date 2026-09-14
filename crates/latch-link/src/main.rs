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
            lifecycle::start(true)?;
        }
        [] => lifecycle::run(false).await?,
        [command] if command == "run" => lifecycle::run(false).await?,
        [command, flag] if command == "run" && flag == "--background" => {
            lifecycle::run(true).await?;
        }
        [command] if command == "start" => lifecycle::start(false)?,
        [command, flag] if command == "start" && flag == "--startup" => lifecycle::start(true)?,
        [command] if command == "stop" => lifecycle::stop(false)?,
        [command] if command == "restart" => {
            lifecycle::stop(true)?;
            lifecycle::start(false)?;
        }
        [command] if command == "status" => lifecycle::print_status()?,
        [command] if command == "doctor" => lifecycle::doctor().await?,
        [command] if command == "reset" => lifecycle::reset()?,
        [command] if command == "supervise" => lifecycle::supervise()?,
        _ => {
            print_help();
            return Err("invalid command".into());
        }
    }
    Ok(())
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
    println!("Latch {}\n\nUsage: latch <command>\n\nCommands:\n  pair <code>  Pair this computer and start Latch\n  start        Start Latch in the background\n  stop         Stop background Latch\n  restart      Restart background Latch\n  status       Show connection status\n  doctor       Check installation and connectivity\n  reset        Remove local identity and credential\n  run          Run in the foreground\n  --version    Print version\n  --help       Print help", env!("CARGO_PKG_VERSION"));
}
