//! Error types for `fujihime-core`.

use std::path::PathBuf;
use thiserror::Error;

/// Unified result alias for core engine operations.
pub type Result<T> = std::result::Result<T, FujihimeError>;

/// Errors produced by the VFS, archive parsers, image decoders, compositor, and transitions.
#[derive(Debug, Error)]
pub enum FujihimeError {
    /// Underlying filesystem I/O failure.
    #[error("I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Generic I/O failure without an associated path.
    #[error("I/O error: {0}")]
    IoRaw(#[from] std::io::Error),

    /// Asset could not be located in any configured layer.
    #[error("asset not found: {name}")]
    AssetNotFound { name: String },

    /// Invalid or corrupt `.xfl` archive.
    #[error("invalid XFL archive ({context}): {reason}")]
    InvalidXfl { context: String, reason: String },

    /// Invalid or corrupt `.wcg` image.
    #[error("invalid WCG image: {reason}")]
    InvalidWcg { reason: String },

    /// Unsupported `.wcg` variant (e.g. non-`0x40` stream mode).
    #[error("unsupported WCG format: format_flags=0x{format_flags:04x}, pixel_flags=0x{pixel_flags:08x}")]
    UnsupportedWcg {
        format_flags: u16,
        pixel_flags: u32,
    },

    /// Invalid or corrupt `.lwg` layered image.
    #[error("invalid LWG image: {reason}")]
    InvalidLwg { reason: String },

    /// Invalid or corrupt `.msk` / `.bmp` image.
    #[error("invalid BMP/MSK image: {reason}")]
    InvalidBmp { reason: String },

    /// Buffer size or dimension mismatch on `Surface` construction.
    #[error("invalid surface dimensions or buffer size: {reason}")]
    InvalidSurface { reason: String },

    /// FreeType 2 font loading or glyph rasterization failure.
    #[error("FreeType error: {reason}")]
    FreeType { reason: String },

    /// Text control-code or layout error.
    #[error("text engine error: {reason}")]
    InvalidText { reason: String },

    /// Audio container (`RIFF 0x6771`) or Ogg Vorbis decoding failure.
    #[error("audio error: {reason}")]
    InvalidAudio { reason: String },

    /// MPEG-1 video decoding or state machine error.
    #[error("video error: {reason}")]
    InvalidVideo { reason: String },

    /// `.gsc` script parsing or disassembly error.
    #[error("invalid GSC script: {reason}")]
    InvalidScript { reason: String },

    /// Script VM runtime error (stack overflow/underflow, invalid layer, etc.).
    #[error("VM runtime error: {reason}")]
    VmError { reason: String },

    /// Save data (`KhDLsave.dat` / `KhDLsave%02d.dat`) parsing or serialization error.
    #[error("invalid save data: {reason}")]
    InvalidSave { reason: String },
}

impl FujihimeError {
    /// Wrap a `std::io::Error` with path context.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
