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
        "    writer: Option<Box<dyn Write + Send>>,%NL%".replace("%NL%", "\n"),
        "    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,%NL%".replace("%NL%", "\n"),
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
]

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"repair marker not found:\n{old[:160]}")
    text = text.replace(old, new, 1)

path.write_text(text, encoding="utf-8")
