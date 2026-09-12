use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("path is outside the workspace: {path}")]
    PathOutsideWorkspace { path: PathBuf },
    #[error("file or directory was not found: {path}")]
    FileNotFound { path: PathBuf },
    #[error("permission denied for path: {path}")]
    PermissionDenied { path: PathBuf, #[source] source: io::Error },
    #[error("invalid workspace-relative path: {path}")]
    InvalidPath { path: PathBuf },
    #[error("filesystem operation failed for {path}: {source}")]
    Io { path: PathBuf, #[source] source: io::Error },
}

impl FsError {
    pub(crate) fn from_io(path: PathBuf, source: io::Error) -> Self {
        match source.kind() {
            io::ErrorKind::NotFound => Self::FileNotFound { path },
            io::ErrorKind::PermissionDenied => Self::PermissionDenied { path, source },
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => Self::InvalidPath { path },
            _ => Self::Io { path, source },
        }
    }
}
