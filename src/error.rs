use std::fmt;
use std::io;

/// Errors that can occur while decoding a Shorten file.
#[derive(Debug)]
pub enum ShnError {
    /// The file does not start with the Shorten magic bytes `ajkg`.
    InvalidMagic,
    /// The file version is not supported (only v1-v3 are supported).
    UnsupportedVersion(u8),
    /// The file type is not a supported PCM format.
    UnsupportedFileType(i32),
    /// An unrecognized command was encountered in the bitstream.
    InvalidCommand(i32),
    /// The block size read from the stream is invalid (zero or too large).
    InvalidBlockSize(i32),
    /// Expected a WAVE header in the first verbatim block but did not find one.
    MissingWaveHeader,
    /// The LPC order exceeds the maximum allowed value.
    InvalidLpcOrder(i32),
    /// A wrapped I/O error.
    Io(io::Error),
}

impl fmt::Display for ShnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShnError::InvalidMagic => write!(f, "not a Shorten file (invalid magic)"),
            ShnError::UnsupportedVersion(v) => write!(f, "unsupported Shorten version: {v}"),
            ShnError::UnsupportedFileType(t) => write!(f, "unsupported file type: {t}"),
            ShnError::InvalidCommand(c) => write!(f, "invalid command: {c}"),
            ShnError::InvalidBlockSize(s) => write!(f, "invalid block size: {s}"),
            ShnError::MissingWaveHeader => write!(f, "no WAVE header found in verbatim block"),
            ShnError::InvalidLpcOrder(o) => write!(f, "invalid LPC order: {o}"),
            ShnError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for ShnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShnError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ShnError {
    fn from(e: io::Error) -> Self {
        ShnError::Io(e)
    }
}
