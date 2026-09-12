use std::error::Error;

use latch_link::{LinkClient, LinkConfig};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_logging()?;

    let config = LinkConfig::from_env()?;
    let mut client = LinkClient::new(config)?;
    info!(
        device_id = %client.identity().device_id,
        device_name = %client.identity().device_name,
        "starting Latch Link"
    );

    tokio::select! {
        result = client.run() => result?,
        signal = tokio::signal::ctrl_c() => signal?,
    }

    client.shutdown();
    Ok(())
}

fn init_logging() -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(false)
        .try_init()?;
    Ok(())
}
