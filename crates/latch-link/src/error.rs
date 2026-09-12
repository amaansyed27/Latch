use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("required environment variable {name} is not set")]
    MissingEnvironment { name: &'static str },
    #[error("{name} must not be empty")]
    EmptyEnvironment { name: &'static str },
    #[error("device name must be at most 128 characters")]
    DeviceNameTooLong,
    #[error("router URL is invalid: {0}")]
    InvalidRouterUrl(#[from] url::ParseError),
    #[error("router URL must use http, https, ws, or wss")]
    UnsupportedRouterScheme,
    #[error("could not determine the local application-data directory")]
    ApplicationDataUnavailable,
    #[error("failed to access device identity at {path}: {source}")]
    IdentityIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("device identity at {path} is invalid: {source}")]
    InvalidIdentity {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("device identity at {path} has unsupported version {version}")]
    UnsupportedIdentityVersion { path: PathBuf, version: u8 },
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("router handshake timed out")]
    HandshakeTimeout,
    #[error("router closed the connection during handshake")]
    HandshakeClosed,
    #[error("router rejected the connection: {code}: {message}")]
    HandshakeRejected { code: String, message: String },
    #[error("router sent an invalid link message")]
    InvalidLinkMessage,
    #[error("failed to serialize a link message: {0}")]
    Serialization(#[from] serde_json::Error),
}
