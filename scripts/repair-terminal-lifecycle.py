from pathlib import Path

path = Path("crates/latch-terminal/src/lib.rs")
text = path.read_text()

replacements = [
    ("            master: pair.master,\n", "            master: Some(pair.master),\n"),
    ("            writer,\n", "            writer: Some(writer),\n"),
    ("            _job: job,\n", "            _job: Some(job),\n"),
    ("    master: Box<dyn MasterPty + Send>,\n", "    master: Option<Box<dyn MasterPty + Send>>,\n"),
    ("    writer: Box<dyn Write + Send>,\n", "    writer: Option<Box<dyn Write + Send>>,\n"),
    ("    _job: win32job::Job,\n", "    _job: Option<win32job::Job>,\n"),
    (
        "        terminal.writer.write_all(text.as_bytes())?;\n        terminal.writer.flush()?;\n",
        "        let writer = terminal\n            .writer\n            .as_mut()\n            .ok_or(TerminalError::NotRunning(terminal_id))?;\n        writer.write_all(text.as_bytes())?;\n        writer.flush()?;\n",
    ),
    (
        "        terminal\n            .master\n            .resize(PtySize {\n",
        "        terminal\n            .master\n            .as_ref()\n            .ok_or(TerminalError::NotRunning(terminal_id))?\n            .resize(PtySize {\n",
    ),
    (
        "    fn kill(&mut self) -> Result<(), TerminalError> {\n        if matches!(self.state()?, TerminalState::Running) {\n            self.child.kill()?;\n        }\n        self.killed = true;\n        if let Some(reader) = self.reader_task.take() {\n            let _ = reader.join();\n        }\n        Ok(())\n    }\n",
        "    fn kill(&mut self) -> Result<(), TerminalError> {\n        if matches!(self.state()?, TerminalState::Running) {\n            self.child.kill()?;\n            #[cfg(windows)]\n            self._job.take();\n            let _ = self.child.wait()?;\n        }\n        self.killed = true;\n        self.writer.take();\n        self.master.take();\n        if let Some(reader) = self.reader_task.take() {\n            let _ = reader.join();\n        }\n        Ok(())\n    }\n",
    ),
]

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"expected source fragment not found: {old[:100]!r}")
    text = text.replace(old, new, 1)

path.write_text(text)
