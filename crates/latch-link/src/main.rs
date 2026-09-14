use std::error::Error;

use latch_link::{LinkClient, LinkConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_logging()?;

    let config = LinkConfig::from_env()?;
    if let [_, command, code] = std::env::args().collect::<Vec<_>>().as_slice() {
        if command == "pair" {
            let identity = LinkClient::pair(&config, code).await?;
            println!(
                "Latch\nPaired: {}\nDevice ID: {}\nCredential stored in the OS credential manager.",
                identity.device_name, identity.device_id
            );
            return Ok(());
        }
    }
    let mut client = LinkClient::new(config)?;
    println!(
        "Latch\nDevice: {}\nDevice ID: {}\nConnecting…",
        client.identity().device_name,
        client.identity().device_id
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
    if std::env::var("LATCH_LOG").as_deref() == Ok("json") {
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
