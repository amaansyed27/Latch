use std::io::{self, BufRead, BufWriter, Write};

use latch_engine::Engine;
use latch_protocol::{ErrorCode, ProtocolError, RequestEnvelope, ResponseEnvelope};
use tracing::warn;

pub fn run(engine: &mut Engine) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = handle_line(engine, &line);
        serde_json::to_writer(&mut stdout, &response).map_err(io::Error::other)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_line(engine: &mut Engine, line: &str) -> ResponseEnvelope {
    let request = match serde_json::from_str::<RequestEnvelope>(line) {
        Ok(request) => request,
        Err(parse_error) => {
            warn!(error = %parse_error, "invalid daemon request");
            return ResponseEnvelope::error(
                None,
                ProtocolError {
                    code: ErrorCode::InvalidRequest,
                    message: "request was not valid Latch protocol JSON".to_owned(),
                },
            );
        }
    };

    engine.handle_envelope(request)
}
