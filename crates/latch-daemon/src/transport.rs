use std::io::{self, BufRead, BufWriter, Write};

use latch_protocol::{
    ErrorCode, ProtocolError, RequestEnvelope, ResponseEnvelope, PROTOCOL_VERSION,
};
use tracing::{error, warn};

use crate::engine::Engine;

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

    if request.version != PROTOCOL_VERSION {
        return ResponseEnvelope::error(
            Some(request.id),
            ProtocolError {
                code: ErrorCode::UnsupportedVersion,
                message: format!(
                    "protocol version {} is unsupported; expected {PROTOCOL_VERSION}",
                    request.version
                ),
            },
        );
    }

    let id = request.id;
    match engine.handle(request.request) {
        Ok(result) => ResponseEnvelope::success(id, result),
        Err(protocol_error) => {
            if protocol_error.code == ErrorCode::Io {
                error!(code = ?protocol_error.code, "serious internal operation error");
            }
            ResponseEnvelope::error(Some(id), protocol_error)
        }
    }
}
