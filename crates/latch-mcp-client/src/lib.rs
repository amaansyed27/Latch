use std::{collections::BTreeMap, env, time::Duration};

use latch_local::{McpServerConfig, McpTransportConfig};
use rmcp::{
    model::CallToolRequestParams,
    transport::{StreamableHttpClientTransport, TokioChildProcess, Transport},
    RoleClient, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::process::Command;

const MCP_TIMEOUT: Duration = Duration::from_secs(30);

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

pub fn list_tools(server: &McpServerConfig) -> Result<Vec<RemoteTool>, McpClientError> {
    run(server, Operation::List).and_then(|result| match result {
        OperationResult::Tools(tools) => Ok(tools),
        OperationResult::Call(_) => Err(McpClientError::Protocol(
            "unexpected MCP operation result".to_owned(),
        )),
    })
}

pub fn call_tool(
    server: &McpServerConfig,
    tool_name: &str,
    arguments: Value,
) -> Result<RemoteCallResult, McpClientError> {
    let arguments = match arguments {
        Value::Object(arguments) => arguments,
        _ => {
            return Err(McpClientError::Protocol(
                "tool arguments must be a JSON object".to_owned(),
            ));
        }
    };
    run(
        server,
        Operation::Call {
            tool_name: tool_name.to_owned(),
            arguments,
        },
    )
    .and_then(|result| match result {
        OperationResult::Call(result) => Ok(result),
        OperationResult::Tools(_) => Err(McpClientError::Protocol(
            "unexpected MCP operation result".to_owned(),
        )),
    })
}

pub fn test_connection(server: &McpServerConfig) -> Result<usize, McpClientError> {
    list_tools(server).map(|tools| tools.len())
}

fn run(server: &McpServerConfig, operation: Operation) -> Result<OperationResult, McpClientError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(McpClientError::Runtime)?;
    runtime.block_on(async {
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
                execute(transport, operation).await
            }
            McpTransportConfig::Http { url } => {
                let transport = StreamableHttpClientTransport::from_uri(url.clone());
                execute(transport, operation).await
            }
        }
    })
}

async fn execute<T>(transport: T, operation: Operation) -> Result<OperationResult, McpClientError>
where
    T: Transport<RoleClient> + Send + 'static,
{
    let service = tokio::time::timeout(MCP_TIMEOUT, ().serve(transport))
        .await
        .map_err(|_| McpClientError::Timeout)?
        .map_err(|error| McpClientError::Connect(error.to_string()))?;

    let outcome = match operation {
        Operation::List => {
            let listed = tokio::time::timeout(MCP_TIMEOUT, service.list_tools(Option::default()))
                .await
                .map_err(|_| McpClientError::Timeout)?
                .map_err(|error| McpClientError::Protocol(error.to_string()))?;
            let value = serde_json::to_value(listed)
                .map_err(|error| McpClientError::Protocol(error.to_string()))?;
            OperationResult::Tools(parse_tools(&value)?)
        }
        Operation::Call {
            tool_name,
            arguments,
        } => {
            let params = CallToolRequestParams::new(tool_name).with_arguments(arguments);
            let called = tokio::time::timeout(MCP_TIMEOUT, service.call_tool(params))
                .await
                .map_err(|_| McpClientError::Timeout)?
                .map_err(|error| McpClientError::Protocol(error.to_string()))?;
            OperationResult::Call(RemoteCallResult {
                result: serde_json::to_value(called)
                    .map_err(|error| McpClientError::Protocol(error.to_string()))?,
            })
        }
    };
    service
        .cancel()
        .await
        .map_err(|error| McpClientError::Protocol(error.to_string()))?;
    Ok(outcome)
}

fn parse_tools(value: &Value) -> Result<Vec<RemoteTool>, McpClientError> {
    let tools = value
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpClientError::Protocol("MCP list_tools omitted tools".to_owned()))?;
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

#[derive(Debug)]
enum Operation {
    List,
    Call {
        tool_name: String,
        arguments: Map<String, Value>,
    },
}

#[derive(Debug)]
enum OperationResult {
    Tools(Vec<RemoteTool>),
    Call(RemoteCallResult),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_schema_without_exposing_transport_details() {
        let value = serde_json::json!({
            "tools": [{
                "name": "echo",
                "description": "Echo text",
                "inputSchema": {"type":"object"}
            }]
        });
        let tools = parse_tools(&value).unwrap();
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description.as_deref(), Some("Echo text"));
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
