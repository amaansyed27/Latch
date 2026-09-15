use std::{
    io::Read,
    path::{Path, PathBuf},
};

use cap_std::{ambient_authority, fs::Dir};
use latch_core::Workspace;

use crate::{
    error::FsError,
    path::{reject_known_escape, validate_relative},
};

const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const DEFAULT_IGNORED_DIRECTORIES: [&str; 5] = [".git", "node_modules", "target", "dist", "build"];

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
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRead {
    pub contents: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    pub query: String,
    pub filename: bool,
    pub content: bool,
    pub glob: Option<String>,
    pub max_results: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: String,
    pub line: Option<usize>,
    pub preview: Option<String>,
    pub kind: SearchMatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchMatchKind {
    Filename,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
    pub files_scanned: usize,
}

#[derive(Debug)]
pub struct WorkspaceFs {
    workspace: Workspace,
    dir: Dir,
}

impl WorkspaceFs {
    pub fn new(workspace: Workspace) -> Result<Self, FsError> {
        let dir = Dir::open_ambient_dir(workspace.root(), ambient_authority())
            .map_err(|source| FsError::from_io(workspace.root().to_path_buf(), source))?;
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

    pub fn read_text_bounded(
        &self,
        path: impl AsRef<Path>,
        max_bytes: usize,
    ) -> Result<TextRead, FsError> {
        let path = self.checked(path.as_ref(), false)?;
        let file = self
            .dir
            .open(&path)
            .map_err(|source| FsError::from_io(path.clone(), source))?;
        let limit = u64::try_from(max_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bytes = Vec::new();
        file.take(limit)
            .read_to_end(&mut bytes)
            .map_err(|source| FsError::from_io(path.clone(), source))?;
        let truncated = bytes.len() > max_bytes;
        if truncated {
            bytes.truncate(max_bytes);
        }
        let contents =
            String::from_utf8(bytes).map_err(|_| FsError::InvalidPath { path: path.clone() })?;
        Ok(TextRead {
            contents,
            truncated,
        })
    }

    pub fn write_text(&self, path: impl AsRef<Path>, contents: &str) -> Result<(), FsError> {
        self.write_text_with_mode(path, contents, true)
    }

    pub fn write_text_with_mode(
        &self,
        path: impl AsRef<Path>,
        contents: &str,
        overwrite: bool,
    ) -> Result<(), FsError> {
        let path = self.checked(path.as_ref(), false)?;
        if !overwrite
            && self
                .dir
                .try_exists(&path)
                .map_err(|source| FsError::from_io(path.clone(), source))?
        {
            return Err(FsError::AlreadyExists { path });
        }
        self.dir
            .write(&path, contents.as_bytes())
            .map_err(|source| FsError::from_io(path, source))
    }

    pub fn apply_replacements(
        &self,
        path: impl AsRef<Path>,
        replacements: &[Replacement],
    ) -> Result<(), FsError> {
        let path = self.checked(path.as_ref(), false)?;
        let mut contents = self
            .dir
            .read_to_string(&path)
            .map_err(|source| FsError::from_io(path.clone(), source))?;
        for replacement in replacements {
            if replacement.old.is_empty() {
                return Err(FsError::PatchConflict {
                    path: path.clone(),
                    message: "old text must not be empty".to_owned(),
                });
            }
            let matches = contents.match_indices(&replacement.old).count();
            if matches != 1 {
                return Err(FsError::PatchConflict {
                    path: path.clone(),
                    message: format!(
                        "expected old text to match exactly once, found {matches} matches"
                    ),
                });
            }
            contents = contents.replacen(&replacement.old, &replacement.new, 1);
        }
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

    pub fn move_path(
        &self,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        overwrite: bool,
    ) -> Result<(), FsError> {
        let from = self.checked(from.as_ref(), false)?;
        let to = self.checked(to.as_ref(), false)?;
        if !overwrite
            && self
                .dir
                .try_exists(&to)
                .map_err(|source| FsError::from_io(to.clone(), source))?
        {
            return Err(FsError::AlreadyExists { path: to });
        }
        if overwrite
            && self
                .dir
                .try_exists(&to)
                .map_err(|source| FsError::from_io(to.clone(), source))?
        {
            self.dir
                .remove_file(&to)
                .map_err(|source| FsError::from_io(to.clone(), source))?;
        }
        self.dir
            .rename(&from, &self.dir, &to)
            .map_err(|source| FsError::from_io(from, source))
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
        let modified_ms = metadata.modified().ok().and_then(|modified| {
            modified
                .into_std()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        });

        Ok(FileMetadata {
            kind,
            len: metadata.len(),
            modified_ms,
        })
    }

    pub fn search(&self, options: &SearchOptions) -> Result<SearchResult, FsError> {
        let mut state = SearchState {
            options,
            matches: Vec::new(),
            truncated: false,
            files_scanned: 0,
        };
        self.search_directory(Path::new("."), &mut state)?;
        Ok(SearchResult {
            matches: state.matches,
            truncated: state.truncated,
            files_scanned: state.files_scanned,
        })
    }

    fn search_directory(
        &self,
        directory: &Path,
        state: &mut SearchState<'_>,
    ) -> Result<(), FsError> {
        if state.truncated {
            return Ok(());
        }
        for entry in self.list_dir(directory)? {
            let path = if directory == Path::new(".") {
                PathBuf::from(&entry.name)
            } else {
                directory.join(&entry.name)
            };
            match entry.kind {
                EntryKind::Directory => {
                    if !DEFAULT_IGNORED_DIRECTORIES.contains(&entry.name.as_str()) {
                        self.search_directory(&path, state)?;
                    }
                }
                EntryKind::File => self.search_file(&path, state)?,
                EntryKind::Symlink | EntryKind::Other => {}
            }
            if state.truncated {
                break;
            }
        }
        Ok(())
    }

    fn search_file(&self, path: &Path, state: &mut SearchState<'_>) -> Result<(), FsError> {
        let display = path.to_string_lossy().replace('\\', "/");
        if state
            .options
            .glob
            .as_deref()
            .is_some_and(|pattern| !wildcard_match(pattern, &display))
        {
            return Ok(());
        }
        state.files_scanned += 1;
        let query = state.options.query.to_lowercase();
        if state.options.filename
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().to_lowercase().contains(&query))
        {
            state.push(SearchMatch {
                path: display.clone(),
                line: None,
                preview: None,
                kind: SearchMatchKind::Filename,
            });
        }
        if state.truncated || !state.options.content {
            return Ok(());
        }
        if self.metadata(path)?.len > MAX_SEARCH_FILE_BYTES {
            return Ok(());
        }
        let Ok(contents) = self.read_text(path) else {
            return Ok(());
        };
        for (index, line) in contents.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                state.push(SearchMatch {
                    path: display.clone(),
                    line: Some(index + 1),
                    preview: Some(line.chars().take(240).collect()),
                    kind: SearchMatchKind::Content,
                });
                if state.truncated {
                    break;
                }
            }
        }
        Ok(())
    }

    fn checked(&self, input: &Path, allow_root: bool) -> Result<PathBuf, FsError> {
        let relative = validate_relative(input, allow_root)?;
        reject_known_escape(&self.workspace, &relative)?;
        Ok(relative)
    }
}

struct SearchState<'a> {
    options: &'a SearchOptions,
    matches: Vec<SearchMatch>,
    truncated: bool,
    files_scanned: usize,
}

impl SearchState<'_> {
    fn push(&mut self, value: SearchMatch) {
        if self.matches.len() >= self.options.max_results.max(1) {
            self.truncated = true;
            return;
        }
        self.matches.push(value);
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v, mut star, mut retry) = (0_usize, 0_usize, None, 0_usize);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&value[v])) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            retry += 1;
            v = retry;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_filter_supports_common_globs() {
        assert!(wildcard_match("*.rs", "src/main.rs"));
        assert!(wildcard_match("src/*", "src/main.rs"));
        assert!(!wildcard_match("*.ts", "src/main.rs"));
    }
}
