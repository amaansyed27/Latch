use std::io;

use latch_core::ProcessId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("command was not found: {program}")]
    CommandNotFound { program: String },
    #[error("process was not found: {process_id}")]
    ProcessNotFound { process_id: ProcessId },
    #[error("failed to start or control process: {message}")]
    ProcessFailed {
        message: String,
        #[source]
        source: io::Error,
    },
    #[error("command execution failed: {message}")]
    Io {
        message: String,
        #[source]
        source: io::Error,
    },
}

impl ExecError {
    pub(crate) fn spawn(program: &str, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            Self::CommandNotFound {
                program: program.to_owned(),
            }
        } else {
            Self::Io {
                message: format!("could not start {program}"),
                source,
            }
        }
    }
}
