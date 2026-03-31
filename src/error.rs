#![deny(warnings)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TimeclockError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    #[error("MCP protocol error: {0}")]
    Mcp(#[from] McpError),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Failed to read data file '{0}': {1}")]
    ReadError(String, String),

    #[error("Failed to write data file '{0}': {1}")]
    WriteError(String, String),

    #[error("Failed to create data directory '{0}': {1}")]
    CreateDirError(String, String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Invalid project ID: {0}")]
    InvalidProjectId(String),
}

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Already clocked in for project '{0}'")]
    AlreadyClockedIn(String),

    #[error("Not clocked in for project '{0}'")]
    NotClockedIn(String),

    #[error("time_out must be >= time_in")]
    TimeOutBeforeTimeIn,

    #[error("Invalid timestamp '{0}': {1}")]
    InvalidTimestamp(String, String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Project '{0}' has {1} session(s); pass delete_entries=true to also remove them")]
    ProjectHasEntries(String, usize),
}

#[derive(Error, Debug)]
pub enum McpError {
    #[error("Unsupported protocol version: {0}")]
    InvalidProtocolVersion(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Invalid tool parameters: {0}")]
    InvalidToolParameters(String),
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Transport IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
}

pub type Result<T> = std::result::Result<T, TimeclockError>;
