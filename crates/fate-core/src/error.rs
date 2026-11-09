//! Typed errors shared by the launcher core.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, FateError>;

#[derive(Debug, thiserror::Error)]
pub enum FateError {
    #[error("{kind} at {path}: {source}")]
    Io {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not parse {kind} at {path}: {source}")]
    Parse {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("no Java runtime usable for Minecraft 1.20.1 was found: {0}")]
    JavaNotFound(String),

    #[error("{0}")]
    Message(String),
}

impl FateError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        FateError::Io {
            kind: "I/O error",
            path: path.into(),
            source,
        }
    }

    pub fn read(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        FateError::Io {
            kind: "read error",
            path: path.into(),
            source,
        }
    }

    pub fn write(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        FateError::Io {
            kind: "write error",
            path: path.into(),
            source,
        }
    }

    pub fn parse(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        FateError::Parse {
            kind: "JSON file",
            path: path.into(),
            source,
        }
    }

    pub fn msg(msg: impl Into<String>) -> Self {
        FateError::Message(msg.into())
    }
}

impl From<String> for FateError {
    fn from(value: String) -> Self {
        FateError::Message(value)
    }
}

impl From<&str> for FateError {
    fn from(value: &str) -> Self {
        FateError::Message(value.to_string())
    }
}
