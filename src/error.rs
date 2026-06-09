use std::io;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum S7Error {
    #[error(transparent)]
    IoErr(#[from] Arc<io::Error>),

    /// Connection refused
    #[error("Connection refused to {host}:{port}")]
    ConnectionRefused {
        host: String,
        port: u16,
    },

    /// Connection timeout
    #[error("Connection timeout to {host}:{port}")]
    ConnectionTimeout {
        host: String,
        port: u16,
    },

    /// Connection closed unexpectedly
    #[error("Connection closed unexpectedly")]
    ConnectionClosed,

    /// Connection not established
    #[error("Not connected")]
    NotConnected,

    /// Operation timeout
    #[error("Operation timeout after {0}ms")]
    Timeout(u64),


    /// S7 protocol error
    #[error("S7 protocol error: {code:#010x} - {message}")]
    ProtocolError {
        code: u32,
        message: String,
    },

    /// PDU parse error
    #[error("PDU parse error at offset {offset}: {message}")]
    PduParseError {
        offset: usize,
        message: String,
    },

    /// Invalid memory area
    #[error("Invalid memory area: {0:#04x}")]
    InvalidArea(u8),

    /// Data length mismatch
    #[error("Data length mismatch: expected {expected}, got {actual}")]
    DataLengthMismatch {
        expected: usize,
        actual: usize,
    },

    #[error("buffer too short: need {need} bytes, have {have}")]
    BufferTooShort { need: usize, have: usize },

    #[error("Error: {0}")]
    Error(String),
}

impl S7Error {
    /// Check if this is a connection error
    pub fn is_connection_error(&self) -> bool {
        matches!(
            self,
            Self::ConnectionRefused { .. }
                | Self::ConnectionTimeout { .. }
                | Self::ConnectionClosed
                | Self::NotConnected
        )
    }
}

pub type Result<T> = std::result::Result<T, S7Error>;


// 从 io::Error 自动转换
impl From<io::Error> for S7Error {
    fn from(e: io::Error) -> Self {
        S7Error::IoErr(Arc::new(e))
    }
}