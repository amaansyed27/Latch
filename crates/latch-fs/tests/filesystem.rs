use std::{fs, path::Path};

use latch_core::Workspace;
use latch_fs::{EntryKind, FsError, WorkspaceFs};
use tempfile::TempDir;

fn fixture() -> (TempDir, WorkspaceFs) {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let workspace = Workspace::open(temp.path()).expect("temporary directory should open");
    let fs = WorkspaceFs::new(workspace).expect("workspace filesystem should open");
    (temp, fs)
}

#[test]
fn opens_valid_workspace_and_reads_file() {
    let (temp, workspace_fs) = fixture();
    fs::write(temp.path().join("hello.txt"), "hello").expect("fixture write should succeed");

    assert_eq!(workspace_fs.read_text("hello.txt").unwrap(), "hello");
}

#[test]
fn creates_overwrites_lists_and_deletes() {
    let (_temp, workspace_fs) = fixture();

    workspace_fs.create_dir("nested/deeper").unwrap();
    workspace_fs.write_text("nested/deeper/file.txt", "one").unwrap();
    workspace_fs.write_text("nested/deeper/file.txt", "two").unwrap();

    assert_eq!(workspace_fs.read_text("nested/deeper/file.txt").unwrap(), "two");
    let entries = workspace_fs.list_dir("nested/deeper").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "file.txt");
    assert_eq!(entries[0].kind, EntryKind::File);
    assert!(workspace_fs.exists("nested/deeper/file.txt").unwrap());

    workspace_fs.delete_file("nested/deeper/file.txt").unwrap();
    assert!(!workspace_fs.exists("nested/deeper/file.txt").unwrap());
}

#[test]
fn rejects_parent_escape() {
    let (_temp, workspace_fs) = fixture();
    let error = workspace_fs.read_text("../outside.txt").unwrap_err();
    assert!(matches!(error, FsError::PathOutsideWorkspace { .. }));
}

#[test]
fn rejects_absolute_path() {
    let (temp, workspace_fs) = fixture();
    let outside = temp.path().parent().unwrap_or_else(|| Path::new("/")).join("outside.txt");
    let error = workspace_fs.read_text(&outside).unwrap_err();
    assert!(matches!(error, FsError::PathOutsideWorkspace { .. }));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let (temp, workspace_fs) = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(outside.path(), temp.path().join("escape")).unwrap();

    let error = workspace_fs.read_text("escape/secret.txt").unwrap_err();
    assert!(matches!(error, FsError::PathOutsideWorkspace { .. }));
}

#[cfg(windows)]
#[test]
fn rejects_symlink_escape_when_symlinks_are_available() {
    use std::os::windows::fs::symlink_dir;

    let (temp, workspace_fs) = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();

    if symlink_dir(outside.path(), temp.path().join("escape")).is_err() {
        return;
    }

    let error = workspace_fs.read_text("escape/secret.txt").unwrap_err();
    assert!(matches!(error, FsError::PathOutsideWorkspace { .. }));
}
