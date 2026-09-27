use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidManifest,
    UnsafePath,
    UnknownArtifact,
    Execution,
}

#[derive(Debug)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::InvalidManifest => "invalid_manifest",
            ErrorKind::UnsafePath => "unsafe_path",
            ErrorKind::UnknownArtifact => "unknown_artifact",
            ErrorKind::Execution => "execution_error",
        }
    }

    pub fn exit_code(&self) -> i32 {
        2
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for AppError {}
