use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, HashMap},
    env,
    hash::{Hash, Hasher},
    sync::{mpsc, Mutex, MutexGuard, PoisonError},
    thread::{self, JoinHandle},
    time::Duration,
};

use latch_core::{McpServerId, ToolRefId};
use latch_local::{McpServerConfig, McpTransportConfig};
use rmcp::{
    model::CallToolRequestParams,
    service::{RoleClient, RunningService},
    transport::{StreamableHttpClientTransport, TokioChildProcess},
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::process::Command;

const MCP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SEARCH_RESULTS: usize = 5;
const MAX_CATALOGUE_TOOLS: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteCallResult {
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpProviderInfo {
    pub server_id: McpServerId,
    pub display_name: String,
    pub connected: bool,
    pub cached_tools: usize,
    pub catalogue_version: u64,
    pub catalogue_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCandidate {
    pub tool_ref: ToolRefId,
    pub provider_id: McpServerId,
    pub provider_name: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDescription {
    pub tool_ref: ToolRefId,
    pub provider_id: McpServerId,
    pub provider_name: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

pub struct McpManager {
    sender: mpsc::Sender<ManagerCommand>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl McpManager {
    pub fn new() -> Result<Self, McpClientError> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("latch-mcp".to_owned())
            .spawn(move || run_manager(receiver))
            .map_err(McpClientError::Runtime)?;
        Ok(Self {
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn providers(
        &self,
        servers: &[McpServerConfig],
    ) -> Result<Vec<McpProviderInfo>, McpClientError> {
        self.request(|reply| ManagerCommand::Providers {
            servers: servers.to_vec(),
            reply,
        })
    }

    pub fn search(
        &self,
        servers: &[McpServerConfig],
        query: &str,
        provider_id: Option<McpServerId>,
        max_results: usize,
    ) -> Result<Vec<ToolCandidate>, McpClientError> {
        if query.trim().is_empty() {
            return Err(McpClientError::Protocol(
                "tool search query must not be empty".to_owned(),
            ));
        }
        self.request(|reply| ManagerCommand::Search {
            servers: servers.to_vec(),
            query: query.to_owned(),
            provider_id,
            max_results: max_results.clamp(1, MAX_SEARCH_RESULTS),
            reply,
        })
    }

    pub fn describe(
        &self,
        servers: &[McpServerConfig],
        tool_ref: ToolRefId,
    ) -> Result<ToolDescription, McpClientError> {
        self.request(|reply| ManagerCommand::Describe {
            servers: servers.to_vec(),
            tool_ref,
            reply,
        })
    }

    pub fn call(
        &self,
        servers: &[McpServerConfig],
        tool_ref: ToolRefId,
        arguments: Value,
    ) -> Result<RemoteCallResult, McpClientError> {
        let Value::Object(arguments) = arguments else {
            return Err(McpClientError::Protocol(
                "tool arguments must be a JSON object".to_owned(),
            ));
        };
        self.request(|reply| ManagerCommand::CallRef {
            servers: servers.to_vec(),
            tool_ref,
            arguments,
            reply,
        })
    }

    pub fn list_tools(&self, server: &McpServerConfig) -> Result<Vec<RemoteTool>, McpClientError> {
        self.request(|reply| ManagerCommand::List {
            server: server.clone(),
            reply,
        })
    }

    pub fn call_tool(
        &self,
        server: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> Result<RemoteCallResult, McpClientError> {
        let Value::Object(arguments) = arguments else {
            return Err(McpClientError::Protocol(
                "tool arguments must be a JSON object".to_owned(),
            ));
        };
        self.request(|reply| ManagerCommand::CallName {
            server: server.clone(),
            tool_name: tool_name.to_owned(),
            arguments,
            reply,
        })
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(ManagerCommand::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }

    fn request<T: Send + 'static>(
        &self,
        build: impl FnOnce(mpsc::Sender<Result<T, McpClientError>>) -> ManagerCommand,
    ) -> Result<T, McpClientError> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .send(build(reply))
            .map_err(|_| McpClientError::ManagerStopped)?;
        receiver
            .recv()
            .map_err(|_| McpClientError::ManagerStopped)?
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new().expect("MCP manager worker should start")
    }
}

impl Drop for McpManager {
    fn drop(&mut self) {
        let _ = self.sender.send(ManagerCommand::Shutdown);
        if let Ok(worker) = self.worker.get_mut() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

pub fn list_tools(server: &McpServerConfig) -> Result<Vec<RemoteTool>, McpClientError> {
    let manager = McpManager::new()?;
    let result = manager.list_tools(server);
    manager.shutdown();
    result
}

pub fn call_tool(
    server: &McpServerConfig,
    tool_name: &str,
    arguments: Value,
) -> Result<RemoteCallResult, McpClientError> {
    let manager = McpManager::new()?;
    let result = manager.call_tool(server, tool_name, arguments);
    manager.shutdown();
    result
}

pub fn test_connection(server: &McpServerConfig) -> Result<usize, McpClientError> {
    list_tools(server).map(|tools| tools.len())
}

enum ManagerCommand {
    Providers {
        servers: Vec<McpServerConfig>,
        reply: mpsc::Sender<Result<Vec<McpProviderInfo>, McpClientError>>,
    },
    Search {
        servers: Vec<McpServerConfig>,
        query: String,
        provider_id: Option<McpServerId>,
        max_results: usize,
        reply: mpsc::Sender<Result<Vec<ToolCandidate>, McpClientError>>,
    },
    Describe {
        servers: Vec<McpServerConfig>,
        tool_ref: ToolRefId,
        reply: mpsc::Sender<Result<ToolDescription, McpClientError>>,
    },
    CallRef {
        servers: Vec<McpServerConfig>,
        tool_ref: ToolRefId,
        arguments: Map<String, Value>,
        reply: mpsc::Sender<Result<RemoteCallResult, McpClientError>>,
    },
    List {
        server: McpServerConfig,
        reply: mpsc::Sender<Result<Vec<RemoteTool>, McpClientError>>,
    },
    CallName {
        server: McpServerConfig,
        tool_name: String,
        arguments: Map<String, Value>,
        reply: mpsc::Sender<Result<RemoteCallResult, McpClientError>>,
    },
    Shutdown,
}

struct ManagerState {
    connections: HashMap<McpServerId, Connection>,
    refs: HashMap<ToolRefId, ToolKey>,
}

struct Connection {
    fingerprint: u64,
    service: RunningService<RoleClient, ()>,
    tools: Vec<RemoteTool>,
    catalogue_version: u64,
    catalogue_hash: String,
    display_name: String,
}

#[derive(Clone)]
struct ToolKey {
    server_id: McpServerId,
    tool_name: String,
}

fn run_manager(receiver: mpsc::Receiver<ManagerCommand>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            fail_manager(receiver, McpClientError::Runtime(error));
            return;
        }
    };
    runtime.block_on(async move {
        let mut state = ManagerState {
            connections: HashMap::new(),
            refs: HashMap::new(),
        };
        while let Ok(command) = receiver.recv() {
            match command {
                ManagerCommand::Providers { servers, reply } => {
                    let result = providers(&mut state, &servers).await;
                    let _ = reply.send(result);
                }
                ManagerCommand::Search {
                    servers,
                    query,
                    provider_id,
                    max_results,
                    reply,
                } => {
                    let result =
                        search_tools(&mut state, &servers, &query, provider_id, max_results).await;
                    let _ = reply.send(result);
                }
                ManagerCommand::Describe {
                    servers,
                    tool_ref,
                    reply,
                } => {
                    let result = describe_tool(&mut state, &servers, tool_ref).await;
                    let _ = reply.send(result);
                }
                ManagerCommand::CallRef {
                    servers,
                    tool_ref,
                    arguments,
                    reply,
                } => {
                    let result = call_ref(&mut state, &servers, tool_ref, arguments).await;
                    let _ = reply.send(result);
                }
                ManagerCommand::List { server, reply } => {
                    let result = ensure_connection(&mut state, &server)
                        .await
                        .map(|connection| connection.tools.clone());
                    let _ = reply.send(result);
                }
                ManagerCommand::CallName {
                    server,
                    tool_name,
                    arguments,
                    reply,
                } => {
                    let result = call_name(&mut state, &server, &tool_name, arguments).await;
                    let _ = reply.send(result);
                }
                ManagerCommand::Shutdown => break,
            }
        }
        for (_, connection) in state.connections.drain() {
            let _ = connection.service.cancel().await;
        }
    });
}

fn fail_manager(receiver: mpsc::Receiver<ManagerCommand>, error: McpClientError) {
    while let Ok(command) = receiver.recv() {
        match command {
            ManagerCommand::Providers { reply, .. } => {
                let _ = reply.send(Err(error.clone_without_source()));
            }
            ManagerCommand::Search { reply, .. } => {
                let _ = reply.send(Err(error.clone_without_source()));
            }
            ManagerCommand::Describe { reply, .. } => {
                let _ = reply.send(Err(error.clone_without_source()));
            }
            ManagerCommand::CallRef { reply, .. } | ManagerCommand::CallName { reply, .. } => {
                let _ = reply.send(Err(error.clone_without_source()));
            }
            ManagerCommand::List { reply, .. } => {
                let _ = reply.send(Err(error.clone_without_source()));
            }
            ManagerCommand::Shutdown => return,
        }
    }
}

async fn providers(
    state: &mut ManagerState,
    servers: &[McpServerConfig],
) -> Result<Vec<McpProviderInfo>, McpClientError> {
    prune_removed(state, servers).await;
    let mut result = Vec::new();
    for server in visible_servers(servers) {
        match ensure_connection(state, server).await {
            Ok(connection) => result.push(McpProviderInfo {
                server_id: server.server_id,
                display_name: server.display_name.clone(),
                connected: !connection.service.peer().is_transport_closed(),
                cached_tools: connection.tools.len(),
                catalogue_version: connection.catalogue_version,
                catalogue_hash: connection.catalogue_hash.clone(),
            }),
            Err(_) => result.push(McpProviderInfo {
                server_id: server.server_id,
                display_name: server.display_name.clone(),
                connected: false,
                cached_tools: 0,
                catalogue_version: 0,
                catalogue_hash: String::new(),
            }),
        }
    }
    Ok(result)
}

async fn search_tools(
    state: &mut ManagerState,
    servers: &[McpServerConfig],
    query: &str,
    provider_id: Option<McpServerId>,
    max_results: usize,
) -> Result<Vec<ToolCandidate>, McpClientError> {
    prune_removed(state, servers).await;
    let query = query.trim().to_ascii_lowercase();
    let mut ranked = Vec::new();
    for server in visible_servers(servers)
        .filter(|server| provider_id.is_none_or(|id| id == server.server_id))
    {
        let connection = ensure_connection(state, server).await?;
        for tool in &connection.tools {
            if let Some(score) = relevance(tool, &query) {
                ranked.push((
                    score,
                    server.server_id,
                    server.display_name.clone(),
                    tool.clone(),
                ));
            }
        }
    }
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.3.name.cmp(&right.3.name))
    });
    let mut result = Vec::new();
    for (_, server_id, provider_name, tool) in ranked.into_iter().take(max_results) {
        let tool_ref = state
            .refs
            .iter()
            .find_map(|(reference, key)| {
                (key.server_id == server_id && key.tool_name == tool.name).then_some(*reference)
            })
            .unwrap_or_else(|| {
                let reference = ToolRefId::new();
                state.refs.insert(
                    reference,
                    ToolKey {
                        server_id,
                        tool_name: tool.name.clone(),
                    },
                );
                reference
            });
        result.push(ToolCandidate {
            tool_ref,
            provider_id: server_id,
            provider_name,
            name: tool.name,
            description: tool.description,
        });
    }
    Ok(result)
}

async fn describe_tool(
    state: &mut ManagerState,
    servers: &[McpServerConfig],
    tool_ref: ToolRefId,
) -> Result<ToolDescription, McpClientError> {
    let key = state
        .refs
        .get(&tool_ref)
        .cloned()
        .ok_or(McpClientError::ToolRefNotFound)?;
    let server = find_server(servers, key.server_id)?;
    let connection = ensure_connection(state, server).await?;
    let tool = connection
        .tools
        .iter()
        .find(|tool| tool.name == key.tool_name)
        .ok_or_else(|| McpClientError::ToolNotFound(key.tool_name.clone()))?;
    Ok(ToolDescription {
        tool_ref,
        provider_id: server.server_id,
        provider_name: server.display_name.clone(),
        name: tool.name.clone(),
        description: tool.description.clone(),
        input_schema: tool.input_schema.clone(),
    })
}

async fn call_ref(
    state: &mut ManagerState,
    servers: &[McpServerConfig],
    tool_ref: ToolRefId,
    arguments: Map<String, Value>,
) -> Result<RemoteCallResult, McpClientError> {
    let key = state
        .refs
        .get(&tool_ref)
        .cloned()
        .ok_or(McpClientError::ToolRefNotFound)?;
    let server = find_server(servers, key.server_id)?;
    call_name(state, server, &key.tool_name, arguments).await
}

async fn call_name(
    state: &mut ManagerState,
    server: &McpServerConfig,
    tool_name: &str,
    arguments: Map<String, Value>,
) -> Result<RemoteCallResult, McpClientError> {
    let connection = ensure_connection(state, server).await?;
    if !connection.tools.iter().any(|tool| tool.name == tool_name) {
        return Err(McpClientError::ToolNotFound(tool_name.to_owned()));
    }
    let params = CallToolRequestParams::new(tool_name.to_owned()).with_arguments(arguments);
    let called = tokio::time::timeout(MCP_TIMEOUT, connection.service.peer().call_tool(params))
        .await
        .map_err(|_| McpClientError::Timeout)?
        .map_err(|error| McpClientError::Protocol(error.to_string()))?;
    Ok(RemoteCallResult {
        result: serde_json::to_value(called)
            .map_err(|error| McpClientError::Protocol(error.to_string()))?,
    })
}

async fn ensure_connection<'a>(
    state: &'a mut ManagerState,
    server: &McpServerConfig,
) -> Result<&'a mut Connection, McpClientError> {
    if !server.enabled || !server.allow_remote {
        return Err(McpClientError::ServerDisabled);
    }
    let fingerprint = config_fingerprint(server);
    let replace = state
        .connections
        .get(&server.server_id)
        .is_some_and(|connection| {
            connection.fingerprint != fingerprint || connection.service.peer().is_transport_closed()
        });
    if replace {
        if let Some(connection) = state.connections.remove(&server.server_id) {
            let _ = connection.service.cancel().await;
        }
        state
            .refs
            .retain(|_, key| key.server_id != server.server_id);
    }
    if !state.connections.contains_key(&server.server_id) {
        let service = connect(server).await?;
        let tools = fetch_catalogue(&service).await?;
        let catalogue_hash = catalogue_hash(&tools);
        state.connections.insert(
            server.server_id,
            Connection {
                fingerprint,
                service,
                tools,
                catalogue_version: 1,
                catalogue_hash,
                display_name: server.display_name.clone(),
            },
        );
    }
    state
        .connections
        .get_mut(&server.server_id)
        .ok_or(McpClientError::Connect("connection disappeared".to_owned()))
}

async fn connect(
    server: &McpServerConfig,
) -> Result<RunningService<RoleClient, ()>, McpClientError> {
    match &server.transport {
        McpTransportConfig::Stdio {
            command,
            arguments,
            environment_references,
        } => {
            let mut process = Command::new(command);
            process.args(arguments);
            apply_environment(&mut process, environment_references)?;
            let transport = TokioChildProcess::new(process)
                .map_err(|error| McpClientError::Connect(error.to_string()))?;
            tokio::time::timeout(MCP_TIMEOUT, ().serve(transport))
                .await
                .map_err(|_| McpClientError::Timeout)?
                .map_err(|error| McpClientError::Connect(error.to_string()))
        }
        McpTransportConfig::Http { url } => {
            let transport = StreamableHttpClientTransport::from_uri(url.clone());
            tokio::time::timeout(MCP_TIMEOUT, ().serve(transport))
                .await
                .map_err(|_| McpClientError::Timeout)?
                .map_err(|error| McpClientError::Connect(error.to_string()))
        }
    }
}

async fn fetch_catalogue(
    service: &RunningService<RoleClient, ()>,
) -> Result<Vec<RemoteTool>, McpClientError> {
    let tools = tokio::time::timeout(MCP_TIMEOUT, service.peer().list_all_tools())
        .await
        .map_err(|_| McpClientError::Timeout)?
        .map_err(|error| McpClientError::Protocol(error.to_string()))?;
    if tools.len() > MAX_CATALOGUE_TOOLS {
        return Err(McpClientError::Protocol(format!(
            "MCP catalogue exceeds {MAX_CATALOGUE_TOOLS} tools"
        )));
    }
    let value =
        serde_json::to_value(tools).map_err(|error| McpClientError::Protocol(error.to_string()))?;
    parse_tools_array(&value)
}

async fn prune_removed(state: &mut ManagerState, servers: &[McpServerConfig]) {
    let allowed = visible_servers(servers)
        .map(|server| (server.server_id, config_fingerprint(server)))
        .collect::<HashMap<_, _>>();
    let remove = state
        .connections
        .iter()
        .filter_map(|(server_id, connection)| {
            (!allowed
                .get(server_id)
                .is_some_and(|fingerprint| *fingerprint == connection.fingerprint))
            .then_some(*server_id)
        })
        .collect::<Vec<_>>();
    for server_id in remove {
        if let Some(connection) = state.connections.remove(&server_id) {
            let _ = connection.service.cancel().await;
        }
        state.refs.retain(|_, key| key.server_id != server_id);
    }
}

fn visible_servers(servers: &[McpServerConfig]) -> impl Iterator<Item = &McpServerConfig> {
    servers
        .iter()
        .filter(|server| server.enabled && server.allow_remote)
}

fn find_server(
    servers: &[McpServerConfig],
    server_id: McpServerId,
) -> Result<&McpServerConfig, McpClientError> {
    servers
        .iter()
        .find(|server| server.server_id == server_id && server.enabled && server.allow_remote)
        .ok_or(McpClientError::ServerDisabled)
}

fn parse_tools_array(value: &Value) -> Result<Vec<RemoteTool>, McpClientError> {
    let tools = value
        .as_array()
        .ok_or_else(|| McpClientError::Protocol("MCP catalogue is not an array".to_owned()))?;
    tools
        .iter()
        .map(|tool| {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| McpClientError::Protocol("MCP tool omitted name".to_owned()))?;
            Ok(RemoteTool {
                name: name.to_owned(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                input_schema: tool
                    .get("inputSchema")
                    .or_else(|| tool.get("input_schema"))
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new())),
            })
        })
        .collect()
}

fn relevance(tool: &RemoteTool, query: &str) -> Option<u8> {
    let name = tool.name.to_ascii_lowercase();
    let description = tool
        .description
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name == query {
        Some(4)
    } else if name.starts_with(query) {
        Some(3)
    } else if name.contains(query) {
        Some(2)
    } else if description.contains(query) {
        Some(1)
    } else {
        None
    }
}

fn catalogue_hash(tools: &[RemoteTool]) -> String {
    let mut hasher = DefaultHasher::new();
    for tool in tools {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        tool.input_schema.to_string().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn config_fingerprint(server: &McpServerConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    server.server_id.hash(&mut hasher);
    server.display_name.hash(&mut hasher);
    serde_json::to_string(&server.transport)
        .unwrap_or_default()
        .hash(&mut hasher);
    server.enabled.hash(&mut hasher);
    server.allow_remote.hash(&mut hasher);
    hasher.finish()
}

fn apply_environment(
    command: &mut Command,
    references: &BTreeMap<String, String>,
) -> Result<(), McpClientError> {
    for (target, source) in references {
        let value = env::var(source)
            .map_err(|_| McpClientError::MissingEnvironmentReference(source.clone()))?;
        command.env(target, value);
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Error)]
pub enum McpClientError {
    #[error("local MCP connection failed: {0}")]
    Connect(String),
    #[error("local MCP protocol error: {0}")]
    Protocol(String),
    #[error("local MCP request timed out")]
    Timeout,
    #[error("required environment variable is unavailable: {0}")]
    MissingEnvironmentReference(String),
    #[error("could not create local MCP runtime: {0}")]
    Runtime(#[source] std::io::Error),
    #[error("MCP manager stopped")]
    ManagerStopped,
    #[error("local MCP server is disabled or not remotely allowed")]
    ServerDisabled,
    #[error("local MCP tool was not found: {0}")]
    ToolNotFound(String),
    #[error("opaque local MCP tool reference is unknown or stale")]
    ToolRefNotFound,
}

impl McpClientError {
    fn clone_without_source(&self) -> Self {
        match self {
            Self::Connect(value) => Self::Connect(value.clone()),
            Self::Protocol(value) => Self::Protocol(value.clone()),
            Self::Timeout => Self::Timeout,
            Self::MissingEnvironmentReference(value) => {
                Self::MissingEnvironmentReference(value.clone())
            }
            Self::Runtime(source) => Self::Connect(source.to_string()),
            Self::ManagerStopped => Self::ManagerStopped,
            Self::ServerDisabled => Self::ServerDisabled,
            Self::ToolNotFound(value) => Self::ToolNotFound(value.clone()),
            Self::ToolRefNotFound => Self::ToolRefNotFound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_tools(count: usize) -> Vec<RemoteTool> {
        (0..count)
            .map(|index| RemoteTool {
                name: format!("tool_{index:03}"),
                description: Some(if index % 50 == 0 {
                    "special benchmark target".to_owned()
                } else {
                    "synthetic fixture tool".to_owned()
                }),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"value": {"type":"string"}}
                }),
            })
            .collect()
    }

    #[test]
    fn parses_tool_schema_without_exposing_transport_details() {
        let value = serde_json::json!([{
            "name": "echo",
            "description": "Echo text",
            "inputSchema": {"type":"object"}
        }]);
        let tools = parse_tools_array(&value).unwrap();
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description.as_deref(), Some("Echo text"));
    }

    #[test]
    fn synthetic_250_tool_catalogue_search_is_bounded_and_schema_lazy() {
        let tools = synthetic_tools(250);
        let mut matches = tools
            .iter()
            .filter_map(|tool| relevance(tool, "benchmark").map(|score| (score, tool)))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| right.0.cmp(&left.0));
        let concise = matches
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|(_, tool)| ToolCandidate {
                tool_ref: ToolRefId::new(),
                provider_id: McpServerId::new(),
                provider_name: "fixture".to_owned(),
                name: tool.name.clone(),
                description: tool.description.clone(),
            })
            .collect::<Vec<_>>();
        assert!(concise.len() <= 5);
        let encoded = serde_json::to_value(&concise).unwrap();
        assert!(!encoded.to_string().contains("input_schema"));
    }

    #[test]
    fn missing_environment_references_fail_closed() {
        let mut refs = BTreeMap::new();
        refs.insert(
            "TOKEN".to_owned(),
            "LATCH_TEST_ENV_THAT_SHOULD_NOT_EXIST".to_owned(),
        );
        let mut command = Command::new("ignored");
        assert!(matches!(
            apply_environment(&mut command, &refs),
            Err(McpClientError::MissingEnvironmentReference(_))
        ));
    }
}
