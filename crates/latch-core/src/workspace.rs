use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::WorkspaceId;

#[derive(Debug, Clone)]
pub struct Workspace {
    id: WorkspaceId,
    root: PathBuf,
}

impl Workspace {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        let root = fs::canonicalize(requested).map_err(|source| map_open_error(requested, source))?;
        let metadata = fs::metadata(&root).map_err(|source| map_open_error(requested, source))?;

        if !metadata.is_dir() {
            return Err(WorkspaceError::NotDirectory {
                path: requested.to_path_buf(),
            });
        }

        Ok(Self {
            id: WorkspaceId::new(),
            root,
        })
    }

    pub fn create(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        fs::create_dir_all(requested).map_err(|source| map_open_error(requested, source))?;
        Self::open(requested)
    }

    pub const fn id(&self) -> WorkspaceId {
        self.id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace was not found: {path}")]
    NotFound { path: PathBuf },
    #[error("workspace path is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("permission denied while opening workspace: {path}")]
    PermissionDenied { path: PathBuf, #[source] source: io::Error },
    #[error("failed to open workspace {path}: {source}")]
    Io { path: PathBuf, #[source] source: io::Error },
}

fn map_open_error(path: &Path, source: io::Error) -> WorkspaceError {
    match source.kind() {
        io::ErrorKind::NotFound => WorkspaceError::NotFound {
            path: path.to_path_buf(),
        },
        io::ErrorKind::PermissionDenied => WorkspaceError::PermissionDenied {
            path: path.to_path_buf(),
            source,
        },
        _ => WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        },
    }
}
