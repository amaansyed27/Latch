mod error;
mod path;
mod workspace_fs;

pub use error::FsError;
pub use workspace_fs::{
    DirectoryEntry, EntryKind, FileMetadata, Replacement, SearchMatch, SearchMatchKind,
    SearchOptions, SearchResult, TextRead, WorkspaceFs,
};
