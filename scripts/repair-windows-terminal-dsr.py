from pathlib import Path

path = Path("crates/latch-terminal/src/lib.rs")
text = path.read_text(encoding="utf-8")

replacements = [
    (
        "sync::{Arc, Mutex, MutexGuard, PoisonError},",
        "sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},",
    ),
    (
        '''        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TerminalError::Create(error.to_string()))?;''',
        '''        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|error| TerminalError::Create(error.to_string()))?,
        ));''',
    ),
    (
        "        let reader_task = spawn_reader(reader, Arc::clone(&output));",
        "        let reader_task = spawn_reader(reader, Arc::clone(&output), Arc::downgrade(&writer));",
    ),
    (
        "            _job: Some(job),",
        "            job: Some(job),",
    ),
    (
        "    writer: Option<Box<dyn Write + Send>>,%NL%".replace("%NL%", "\n"),
        "    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,%NL%".replace("%NL%", "\n"),
    ),
    (
        "    _job: Option<win32job::Job>,",
        "    job: Option<win32job::Job>,",
    ),
    (
        "            self._job.take();",
        "            self.job.take();",
    ),
    (
        '''        let writer = terminal
            .writer
            .as_mut()
            .ok_or(TerminalError::NotRunning(terminal_id))?;
        writer.write_all(text.as_bytes())?;
        writer.flush()?;''',
        '''        let writer = terminal
            .writer
            .as_ref()
            .ok_or(TerminalError::NotRunning(terminal_id))?;
        let mut writer = lock(writer);
        writer.write_all(text.as_bytes())?;
        writer.flush()?;''',
    ),
    (
        '''fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<OutputRing>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    lock(&output).complete = true;
                    return;
                }
                Ok(count) => lock(&output).append(&chunk[..count]),
                Err(error) => {
                    warn!(%error, "terminal output reader failed");
                    lock(&output).complete = true;
                    return;
                }
            }
        }
    })
}''',
        '''fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<OutputRing>>,
    writer: Weak<Mutex<Box<dyn Write + Send>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        let mut query_tail = Vec::with_capacity(3);
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    lock(&output).complete = true;
                    return;
                }
                Ok(count) => {
                    let bytes = &chunk[..count];
                    let mut query_scan = Vec::with_capacity(query_tail.len() + bytes.len());
                    query_scan.extend_from_slice(&query_tail);
                    query_scan.extend_from_slice(bytes);
                    if query_scan.windows(4).any(|window| window == b"\\x1b[6n") {
                        if let Some(writer) = writer.upgrade() {
                            let mut writer = lock(&writer);
                            if let Err(error) = writer
                                .write_all(b"\\x1b[1;1R")
                                .and_then(|()| writer.flush())
                            {
                                warn!(%error, "terminal cursor-position response failed");
                            }
                        }
                    }
                    query_tail.clear();
                    let keep = query_scan.len().min(3);
                    query_tail.extend_from_slice(&query_scan[query_scan.len() - keep..]);
                    lock(&output).append(bytes);
                }
                Err(error) => {
                    warn!(%error, "terminal output reader failed");
                    lock(&output).complete = true;
                    return;
                }
            }
        }
    })
}''',
    ),
    (
        '''fn find_git_bash() -> Option<PathBuf> {
    for path in [
        PathBuf::from(r"C:\\Program Files\\Git\\bin\\bash.exe"),
        PathBuf::from(r"C:\\Program Files\\Git\\usr\\bin\\bash.exe"),
    ] {
        if path.is_file() {
            return Some(path);
        }
    }
    None
}''',
        '''fn find_git_bash() -> Option<PathBuf> {
    [
        PathBuf::from(r"C:\\Program Files\\Git\\bin\\bash.exe"),
        PathBuf::from(r"C:\\Program Files\\Git\\usr\\bin\\bash.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}''',
    ),
    (
        "    use std::{thread, time::Duration};",
        "    use std::{thread, time::{Duration, Instant}};",
    ),
    (
        '''        manager
            .write(created.terminal_id, "echo LATCH_PTY_TEST\\r\\n")
            .unwrap();
        thread::sleep(Duration::from_millis(350));
        let first = manager
            .snapshot(created.terminal_id, None, Some(32 * 1024))
            .unwrap();
        assert!(first.logical_screen.contains("LATCH_PTY_TEST"));
        manager
            .write(created.terminal_id, "echo SECOND\\r\\n")
            .unwrap();
        thread::sleep(Duration::from_millis(250));
        let second = manager
            .snapshot(created.terminal_id, Some(first.sequence), Some(32 * 1024))
            .unwrap();
        assert!(second.output.contains("SECOND"));''',
        '''        manager
            .write(created.terminal_id, "echo LATCH_PTY_TEST\\r\\n")
            .unwrap();
        let first_deadline = Instant::now() + Duration::from_secs(5);
        let first = loop {
            let snapshot = manager
                .snapshot(created.terminal_id, None, Some(32 * 1024))
                .unwrap();
            if snapshot.logical_screen.contains("LATCH_PTY_TEST") {
                break snapshot;
            }
            assert!(
                Instant::now() < first_deadline,
                "timed out waiting for first terminal output: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        manager
            .write(created.terminal_id, "echo SECOND\\r\\n")
            .unwrap();
        let second_deadline = Instant::now() + Duration::from_secs(5);
        let second = loop {
            let snapshot = manager
                .snapshot(created.terminal_id, Some(first.sequence), Some(32 * 1024))
                .unwrap();
            if snapshot.output.contains("SECOND") {
                break snapshot;
            }
            assert!(
                Instant::now() < second_deadline,
                "timed out waiting for incremental terminal output: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        assert!(second.output.contains("SECOND"));''',
    ),
]

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"repair marker not found:\n{old[:160]}")
    text = text.replace(old, new, 1)

path.write_text(text, encoding="utf-8")
