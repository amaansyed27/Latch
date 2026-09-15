use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use latch_core::{McpServerId, RootId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

const CONFIG_FILE: &str = "local-config.json";
const ACTIVITY_FILE: &str = "activity.json";
const MAX_ACTIVITY_ENTRIES: usize = 200;
const MAX_ACTIVITY_DETAIL: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovedRoot {
    pub root_id: RootId,
    pub display_name: String,
    pub canonical_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Permissions {
    pub files: bool,
    pub commands: bool,
    pub screen: bool,
    pub computer_control: bool,
    pub mcp_discovery: bool,
    pub mcp_execution: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            files: true,
            commands: true,
            screen: false,
            computer_control: false,
            mcp_discovery: false,
            mcp_execution: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        arguments: Vec<String>,
        #[serde(default)]
        environment_references: BTreeMap<String, String>,
    },
    Http {
        url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub server_id: McpServerId,
    pub display_name: String,
    pub transport: McpTransportConfig,
    pub enabled: bool,
    pub allow_remote: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalConfig {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub legacy_absolute_workspaces: bool,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub roots: Vec<ApprovedRoot>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityEntry {
    pub at_ms: u64,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct LocalStore {
    directory: PathBuf,
}

impl LocalStore {
    pub fn default_location() -> Result<Self, LocalError> {
        if let Some(path) = env::var_os("LATCH_LOCAL_STATE_DIR") {
            return Ok(Self::new(PathBuf::from(path)));
        }
        let directory = dirs::data_local_dir()
            .ok_or(LocalError::ApplicationDataUnavailable)?
            .join("Latch");
        Ok(Self::new(directory))
    }

    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn load(&self) -> Result<LocalConfig, LocalError> {
        let path = self.directory.join(CONFIG_FILE);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|source| LocalError::InvalidConfig { path, source }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LocalConfig::default()),
            Err(source) => Err(LocalError::Io { path, source }),
        }
    }

    pub fn save(&self, config: &LocalConfig) -> Result<(), LocalError> {
        fs::create_dir_all(&self.directory).map_err(|source| LocalError::Io {
            path: self.directory.clone(),
            source,
        })?;
        let path = self.directory.join(CONFIG_FILE);
        let bytes = serde_json::to_vec_pretty(config).map_err(LocalError::Serialize)?;
        fs::write(&path, bytes).map_err(|source| LocalError::Io { path, source })
    }

    pub fn add_root(&self, path: impl AsRef<Path>) -> Result<ApprovedRoot, LocalError> {
        let requested = path.as_ref();
        let canonical_path = fs::canonicalize(requested).map_err(|source| LocalError::Io {
            path: requested.to_path_buf(),
            source,
        })?;
        let metadata = fs::metadata(&canonical_path).map_err(|source| LocalError::Io {
            path: canonical_path.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(LocalError::RootNotDirectory(canonical_path));
        }
        let mut config = self.load()?;
        if let Some(existing) = config
            .roots
            .iter()
            .find(|root| paths_equal(&root.canonical_path, &canonical_path))
        {
            return Ok(existing.clone());
        }
        let root = ApprovedRoot {
            root_id: RootId::new(),
            display_name: root_display_name(&canonical_path),
            canonical_path,
        };
        config.roots.push(root.clone());
        config
            .roots
            .sort_by(|left, right| left.display_name.cmp(&right.display_name));
        self.save(&config)?;
        Ok(root)
    }

    pub fn remove_root(&self, root_id: RootId) -> Result<bool, LocalError> {
        let mut config = self.load()?;
        let before = config.roots.len();
        config.roots.retain(|root| root.root_id != root_id);
        if config.roots.len() == before {
            return Ok(false);
        }
        self.save(&config)?;
        Ok(true)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), LocalError> {
        let mut config = self.load()?;
        config.paused = paused;
        self.save(&config)
    }

    pub fn set_permissions(&self, permissions: Permissions) -> Result<(), LocalError> {
        let mut config = self.load()?;
        config.permissions = permissions;
        self.save(&config)
    }

    pub fn upsert_mcp_server(&self, server: McpServerConfig) -> Result<(), LocalError> {
        validate_mcp_server(&server)?;
        let mut config = self.load()?;
        if let Some(existing) = config
            .mcp_servers
            .iter_mut()
            .find(|candidate| candidate.server_id == server.server_id)
        {
            *existing = server;
        } else {
            config.mcp_servers.push(server);
        }
        config
            .mcp_servers
            .sort_by(|left, right| left.display_name.cmp(&right.display_name));
        self.save(&config)
    }

    pub fn remove_mcp_server(&self, server_id: McpServerId) -> Result<bool, LocalError> {
        let mut config = self.load()?;
        let before = config.mcp_servers.len();
        config
            .mcp_servers
            .retain(|server| server.server_id != server_id);
        if config.mcp_servers.len() == before {
            return Ok(false);
        }
        self.save(&config)?;
        Ok(true)
    }

    pub fn activity(&self) -> Result<Vec<ActivityEntry>, LocalError> {
        let path = self.directory.join(ACTIVITY_FILE);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|source| LocalError::InvalidConfig { path, source }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(source) => Err(LocalError::Io { path, source }),
        }
    }

    pub fn record_activity(&self, action: &str, detail: &str) -> Result<(), LocalError> {
        let mut entries = self.activity()?;
        entries.push(ActivityEntry {
            at_ms: now_ms(),
            action: truncate(action, 80),
            detail: truncate(detail, MAX_ACTIVITY_DETAIL),
        });
        if entries.len() > MAX_ACTIVITY_ENTRIES {
            entries.drain(..entries.len() - MAX_ACTIVITY_ENTRIES);
        }
        fs::create_dir_all(&self.directory).map_err(|source| LocalError::Io {
            path: self.directory.clone(),
            source,
        })?;
        let path = self.directory.join(ACTIVITY_FILE);
        let bytes = serde_json::to_vec_pretty(&entries).map_err(LocalError::Serialize)?;
        fs::write(&path, bytes).map_err(|source| LocalError::Io { path, source })
    }

    pub fn clear_activity(&self) -> Result<(), LocalError> {
        let path = self.directory.join(ACTIVITY_FILE);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(LocalError::Io { path, source }),
        }
    }
}

#[derive(Debug, Error)]
pub enum LocalError {
    #[error("local application data directory is unavailable")]
    ApplicationDataUnavailable,
    #[error("approved root is not a directory: {0}")]
    RootNotDirectory(PathBuf),
    #[error("invalid local MCP configuration: {0}")]
    InvalidMcp(String),
    #[error("invalid local configuration {path}: {source}")]
    InvalidConfig {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not serialize local configuration: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("local configuration I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn validate_mcp_server(server: &McpServerConfig) -> Result<(), LocalError> {
    if server.display_name.trim().is_empty() || server.display_name.chars().count() > 128 {
        return Err(LocalError::InvalidMcp(
            "display name must contain 1 to 128 characters".to_owned(),
        ));
    }
    match &server.transport {
        McpTransportConfig::Stdio {
            command,
            arguments,
            environment_references,
        } => {
            if command.trim().is_empty() || command.chars().count() > 4096 {
                return Err(LocalError::InvalidMcp(
                    "stdio command must contain 1 to 4096 characters".to_owned(),
                ));
            }
            if arguments.len() > 256 || arguments.iter().any(|arg| arg.len() > 8192) {
                return Err(LocalError::InvalidMcp(
                    "stdio arguments are too large".to_owned(),
                ));
            }
            for (target, source) in environment_references {
                if !valid_env_name(target) || !valid_env_name(source) {
                    return Err(LocalError::InvalidMcp(
                        "environment references must contain variable names, not secret values"
                            .to_owned(),
                    ));
                }
            }
        }
        McpTransportConfig::Http { url } => {
            let parsed = Url::parse(url)
                .map_err(|_| LocalError::InvalidMcp("HTTP MCP URL is invalid".to_owned()))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(LocalError::InvalidMcp(
                    "HTTP MCP URL must use http or https".to_owned(),
                ));
            }
            if parsed.host_str().is_none() {
                return Err(LocalError::InvalidMcp(
                    "HTTP MCP URL must include a host".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn root_display_name(path: &Path) -> String {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn valid_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|character| matches!(character, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_canonical_and_deduplicated() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::new(temp.path().join("state"));
        let root_path = temp.path().join("projects");
        fs::create_dir(&root_path).unwrap();
        let first = store.add_root(&root_path).unwrap();
        let second = store.add_root(&root_path).unwrap();
        assert_eq!(first.root_id, second.root_id);
        assert_eq!(store.load().unwrap().roots.len(), 1);
    }

    #[test]
    fn powerful_permissions_default_off() {
        let permissions = Permissions::default();
        assert!(permissions.files);
        assert!(permissions.commands);
        assert!(!permissions.screen);
        assert!(!permissions.computer_control);
        assert!(!permissions.mcp_discovery);
        assert!(!permissions.mcp_execution);
    }

    #[test]
    fn activity_is_bounded_and_clearable() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalStore::new(temp.path());
        for index in 0..220 {
            store
                .record_activity("test", &format!("entry {index}"))
                .unwrap();
        }
        let entries = store.activity().unwrap();
        assert_eq!(entries.len(), MAX_ACTIVITY_ENTRIES);
        assert_eq!(entries.first().unwrap().detail, "entry 20");
        store.clear_activity().unwrap();
        assert!(store.activity().unwrap().is_empty());
    }

    #[test]
    fn mcp_environment_values_are_references_only() {
        let mut refs = BTreeMap::new();
        refs.insert("TOKEN".to_owned(), "MY_TOKEN".to_owned());
        let server = McpServerConfig {
            server_id: McpServerId::new(),
            display_name: "fixture".to_owned(),
            transport: McpTransportConfig::Stdio {
                command: "fixture".to_owned(),
                arguments: Vec::new(),
                environment_references: refs,
            },
            enabled: true,
            allow_remote: true,
        };
        assert!(validate_mcp_server(&server).is_ok());
    }
}
