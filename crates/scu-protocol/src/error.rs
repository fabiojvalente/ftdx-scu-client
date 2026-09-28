//! Protocol errors.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("datagram too short: {0} bytes")]
    TooShort(usize),

    #[error("bad magic: {0:02X?}")]
    BadMagic([u8; 2]),

    #[error("unexpected type field: {0:02X?}")]
    BadType([u8; 2]),

    #[error("length field {declared} does not match datagram size {actual}")]
    LengthMismatch { declared: usize, actual: usize },

    #[error("header consistency check failed: {0}")]
    HeaderMismatch(&'static str),

    #[error("declared body length {declared} does not match payload size {actual}")]
    BodyLengthMismatch { declared: usize, actual: usize },
}
