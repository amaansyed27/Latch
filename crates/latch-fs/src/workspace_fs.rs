use std::path::{Path, PathBuf};

use cap_std::{ambient_authority, fs::Dir};
use latch_core::Workspace;

use crate::{
    error::FsError,
    path::{reject_known_escape, validate_relative},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub kind: EntryKind,
    pub len: u64,
}

#[derive(Debug)]
pub struct WorkspaceFs {
    workspace: Workspace,
    dir: Dir,
}

impl WorkspaceFs {
    pub fn new(workspace: Workspace) -> Result<Self, FsError> {
        let dir = Dir::open_ambient_dir(workspace.root(), ambient_authority()).map_err(|source| {
            FsError::from_io(workspace.root().to_path_buf(), source)
        })?;
        Ok(Self { workspace, dir })
    }

    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn read_text(&self, path: impl AsRef<Path>) -> Result<String, FsError> {
        let path = self.checked(path.as_ref(), false)?;
        self.dir
            .read_to_string(&path)
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn write_text(&self, path: impl AsRef<Path>, contents: &str) -> Result<(), FsError> {
        let path = self.checked(path.as_ref(), false)?;
        self.dir
            .write(&path, contents.as_bytes())
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn delete_file(&self, path: impl AsRef<Path>) -> Result<(), FsError> {
        let path = self.checked(path.as_ref(), false)?;
        self.dir
            .remove_file(&path)
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn create_dir(&self, path: impl AsRef<Path>) -> Result<(), FsError> {
        let path = self.checked(path.as_ref(), false)?;
        self.dir
            .create_dir_all(&path)
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn list_dir(&self, path: impl AsRef<Path>) -> Result<Vec<DirectoryEntry>, FsError> {
        let path = self.checked(path.as_ref(), true)?;
        let entries = self
            .dir
            .read_dir(&path)
            .map_err(|source| FsError::from_io(path.clone(), source))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| FsError::from_io(path.clone(), source))?;
            let file_type = entry
                .file_type()
                .map_err(|source| FsError::from_io(path.clone(), source))?;
            let kind = if file_type.is_file() {
                EntryKind::File
            } else if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::Other
            };

            result.push(DirectoryEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                kind,
            });
        }

        result.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(result)
    }

    pub fn exists(&self, path: impl AsRef<Path>) -> Result<bool, FsError> {
        let path = self.checked(path.as_ref(), false)?;
        self.dir
            .try_exists(&path)
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn metadata(&self, path: impl AsRef<Path>) -> Result<FileMetadata, FsError> {
        let path = self.checked(path.as_ref(), false)?;
        let metadata = self
            .dir
            .metadata(&path)
            .map_err(|source| FsError::from_io(path.clone(), source))?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };

        Ok(FileMetadata {
            kind,
            len: metadata.len(),
        })
    }

    fn checked(&self, input: &Path, allow_root: bool) -> Result<PathBuf, FsError> {
        let relative = validate_relative(input, allow_root)?;
        reject_known_escape(&self.workspace, &relative)?;
        Ok(relative)
    }
}
