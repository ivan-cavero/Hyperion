//! Errors for world I/O and bootstrap.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Failures while reading/writing world data or preparing the server data directory.
#[derive(Debug, Error)]
pub enum WorldError {
    /// Filesystem failure at a known path.
    #[error("io error at {path}: {source}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying OS error.
        source: io::Error,
    },
    /// Gzip compress or decompress failure.
    #[error("gzip: {0}")]
    Gzip(String),
    /// Storage NBT encode/decode failure.
    #[error("nbt: {0}")]
    Nbt(String),
    /// `level.dat` structure is missing required fields or has wrong types.
    #[error("invalid level.dat: {0}")]
    InvalidLevelDat(String),
    /// `level-name` is empty, contains path separators, or would escape the data root.
    #[error("invalid level-name: {0}")]
    InvalidLevelName(String),
}

impl WorldError {
    /// Wraps an [`io::Error`] with the path being accessed.
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
