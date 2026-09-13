use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use latch_core::DeviceId;
use latch_engine::Engine;
use latch_protocol::RequestEnvelope;
use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::{
    default_device_id_path, load_device_credential, load_or_create_device_id,
    store_device_credential, ClientMessage, DeviceIdentity, LinkConfig, LinkError,
    ReconnectBackoff, ServerMessage,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

type SharedEngine = Arc<Mutex<Engine>>;

pub struct LinkClient {
    config: LinkConfig,
    identity: DeviceIdentity,
    engine: SharedEngine,
    backoff: ReconnectBackoff,
    device_credential: Option<String>,
}

impl LinkClient {
    pub fn new(config: LinkConfig) -> Result<Self, LinkError> {
        let identity_path = match config.device_id_path() {
            Some(path) => path.to_path_buf(),
            None => default_device_id_path()?,
        };
        let device_id = load_or_create_device_id(&identity_path)?;
        let identity = DeviceIdentity {
            device_id,
            device_name: config.device_name().to_owned(),
        };
        let device_credential = match load_device_credential(device_id) {
            Ok(value) => value,
            Err(_) if config.pairing_token().is_some() => None,
            Err(error) => return Err(error),
        };
        if device_credential.is_none() && config.pairing_token().is_none() {
            return Err(LinkError::DeviceNotPaired);
        }

        Ok(Self {
            config,
            identity,
            engine: Arc::new(Mutex::new(Engine::new())),
            backoff: ReconnectBackoff::default(),
            device_credential,
        })
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub async fn run(&mut self) -> Result<(), LinkError> {
        loop {
            match self.run_session().await {
                Ok(()) => warn!("router connection closed; reconnecting"),
                Err(error) => warn!(error = %error, "router connection failed; reconnecting"),
            }

            let delay = self.backoff.next_delay();
            info!(
                delay_ms = delay.as_millis(),
                "waiting before router reconnect"
            );
            sleep(delay).await;
        }
    }

    pub fn shutdown(&self) {
        lock_engine(&self.engine).shutdown();
    }

    pub async fn pair(config: &LinkConfig, code: &str) -> Result<DeviceIdentity, LinkError> {
        #[derive(serde::Serialize)]
        struct PairRequest<'a> {
            code: &'a str,
            device_id: DeviceId,
            device_name: &'a str,
        }
        #[derive(serde::Deserialize)]
        struct PairResponse {
            device_credential: String,
        }

        let identity_path = config
            .device_id_path()
            .map_or(default_device_id_path()?, Path::to_path_buf);
        let device_id = load_or_create_device_id(&identity_path)?;
        let response = reqwest::Client::new()
            .post(config.pairing_url()?)
            .json(&PairRequest {
                code,
                device_id,
                device_name: config.device_name(),
            })
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(LinkError::PairingRejected);
        }
        let paired: PairResponse = response.json().await?;
        store_device_credential(device_id, &paired.device_credential)?;
        Ok(DeviceIdentity {
            device_id,
            device_name: config.device_name().to_owned(),
        })
    }

    async fn run_session(&mut self) -> Result<(), LinkError> {
        let (mut socket, _) = connect_async(self.config.router_url().as_str()).await?;

        let hello = ClientMessage::Hello {
            device_id: self.identity.device_id,
            device_name: self.identity.device_name.clone(),
            pairing_token: self.config.pairing_token().map(ToOwned::to_owned),
            device_credential: self.device_credential.clone(),
        };
        socket
            .send(Message::Text(serde_json::to_string(&hello)?.into()))
            .await?;

        let first = timeout(HANDSHAKE_TIMEOUT, socket.next())
            .await
            .map_err(|_| LinkError::HandshakeTimeout)?
            .ok_or(LinkError::HandshakeClosed)??;
        match parse_server_message(first)? {
            ServerMessage::Welcome { device_id } if device_id == self.identity.device_id => {
                self.backoff.reset();
                info!(
                    device_id = %self.identity.device_id,
                    device_name = %self.identity.device_name,
                    "connected to Latch Router"
                );
            }
            ServerMessage::Error { code, message } => {
                return Err(LinkError::HandshakeRejected { code, message });
            }
            _ => return Err(LinkError::InvalidLinkMessage),
        }

        let (mut writer, mut reader) = socket.split();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
        let writer_task = tokio::spawn(async move {
            while let Some(message) = outbound_rx.recv().await {
                writer.send(message).await?;
            }
            Ok::<(), tokio_tungstenite::tungstenite::Error>(())
        });
        let mut requests = JoinSet::new();

        let session_result = loop {
            while requests.try_join_next().is_some() {}

            let Some(message) = reader.next().await else {
                break Ok(());
            };
            let message = match message {
                Ok(message) => message,
                Err(error) => break Err(LinkError::WebSocket(error)),
            };

            match message {
                Message::Text(text) => {
                    let Ok(server_message) = serde_json::from_str::<ServerMessage>(text.as_ref())
                    else {
                        break Err(LinkError::InvalidLinkMessage);
                    };
                    match server_message {
                        ServerMessage::Request {
                            request_id,
                            request,
                        } => spawn_remote_request(
                            &mut requests,
                            &self.engine,
                            &outbound_tx,
                            request_id,
                            request,
                        ),
                        ServerMessage::Error { code, message } => {
                            warn!(%code, %message, "router reported link error");
                        }
                        ServerMessage::Welcome { .. } => {
                            break Err(LinkError::InvalidLinkMessage);
                        }
                    }
                }
                Message::Ping(payload) => {
                    if outbound_tx.send(Message::Pong(payload)).is_err() {
                        break Ok(());
                    }
                }
                Message::Close(_) => break Ok(()),
                Message::Binary(_) => break Err(LinkError::InvalidLinkMessage),
                _ => {}
            }
        };

        requests.abort_all();
        drop(outbound_tx);
        writer_task.abort();
        let _ = writer_task.await;
        session_result
    }
}

pub fn execute_remote(
    engine: &mut Engine,
    request_id: String,
    request: RequestEnvelope,
) -> ClientMessage {
    ClientMessage::Response {
        request_id,
        response: engine.handle_envelope(request),
    }
}

fn spawn_remote_request(
    requests: &mut JoinSet<()>,
    engine: &SharedEngine,
    sender: &mpsc::UnboundedSender<Message>,
    request_id: String,
    request: RequestEnvelope,
) {
    let engine = Arc::clone(engine);
    let sender = sender.clone();
    requests.spawn(async move {
        let execution = tokio::task::spawn_blocking(move || {
            let mut engine = lock_engine(&engine);
            execute_remote(&mut engine, request_id, request)
        })
        .await;

        match execution {
            Ok(response) => match serde_json::to_string(&response) {
                Ok(json) => {
                    let _ = sender.send(Message::Text(json.into()));
                }
                Err(error) => {
                    warn!(error = %error, "failed to serialize remote response");
                }
            },
            Err(error) => {
                warn!(error = %error, "remote execution task failed");
            }
        }
    });
}

fn parse_server_message(message: Message) -> Result<ServerMessage, LinkError> {
    match message {
        Message::Text(text) => {
            serde_json::from_str(text.as_ref()).map_err(|_| LinkError::InvalidLinkMessage)
        }
        Message::Close(_) => Err(LinkError::HandshakeClosed),
        _ => Err(LinkError::InvalidLinkMessage),
    }
}

fn lock_engine(engine: &Mutex<Engine>) -> MutexGuard<'_, Engine> {
    engine.lock().unwrap_or_else(PoisonError::into_inner)
}
