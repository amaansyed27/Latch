mod engine;
mod transport;

use std::error::Error;

use tracing_subscriber::EnvFilter;

use crate::engine::Engine;

fn main() -> Result<(), Box<dyn Error>> {
    init_logging()?;
    transport::run(&mut Engine::new())?;
    Ok(())
}

fn init_logging() -> Result<(), Box<dyn Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init()?;
    Ok(())
}
