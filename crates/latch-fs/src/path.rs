use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use latch_core::Workspace;
use tracing::warn;

use crate::FsError;

pub(crate) fn validate_relative(path: &Path, allow_root: bool) -> Result<PathBuf, FsError> {
    if path.is_absolute() {
        reject(path);
        return Err(FsError::PathOutsideWorkspace {
            path: path.to_path_buf(),
        });
    }

    let mut clean = PathBuf::new();
    let mut has_normal_component = false;

    for component in path.components() {
        match component {
            Component::Normal(value) => {
                has_normal_component = true;
                clean.push(value);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                reject(path);
                return Err(FsError::PathOutsideWorkspace {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    if !has_normal_component && !allow_root {
        return Err(FsError::InvalidPath {
            path: path.to_path_buf(),
        });
    }

    if clean.as_os_str().is_empty() {
        clean.push(".");
    }

    Ok(clean)
}

pub(crate) fn reject_known_escape(workspace: &Workspace, relative: &Path) -> Result<(), FsError> {
    let candidate = workspace.root().join(relative);
    let mut cursor = Some(candidate.as_path());

    while let Some(path) = cursor {
        match fs::canonicalize(path) {
            Ok(resolved) => {
                if !resolved.starts_with(workspace.root()) {
                    reject(relative);
                    return Err(FsError::PathOutsideWorkspace {
                        path: relative.to_path_buf(),
                    });
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                cursor = path.parent();
            }
            Err(error) => {
                reject(relative);
                return Err(FsError::from_io(relative.to_path_buf(), error));
            }
        }
    }

    Err(FsError::InvalidPath {
        path: relative.to_path_buf(),
    })
}

fn reject(path: &Path) {
    warn!(path = %path.display(), "rejected workspace path escape");
}
