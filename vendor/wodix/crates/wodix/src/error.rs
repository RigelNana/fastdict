//! Error types for the wodix crate.

use thiserror::Error;

/// Typed failures from parsing, record access, searching and cache I/O.
///
/// Match variants rather than display strings; new variants may be added.
/// Parsing validates headers and directories, while record checksum and
/// decompression errors may first appear during [`crate::Read::read`].
/// [`crate::Lookup::visit`] can return an error after invoking its visitor:
/// callers must decide whether to discard those partial results.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    /// The source header is malformed or internally inconsistent.
    #[error("invalid header: {0}")]
    InvalidHeader(String),

    /// The source declares an unsupported numeric format version.
    #[error("unsupported version: {0}")]
    UnsupportedVersion(f64),

    /// The source declares an unsupported text encoding.
    #[error("unsupported encoding: {0}")]
    UnsupportedEncoding(String),

    /// The source requests unsupported encryption flags.
    #[error("unsupported encryption flags: {0}")]
    UnsupportedEncryption(u32),

    /// An allocation, input size or work counter exceeded its component budget.
    #[error("{kind} exceeds limit: {actual} > {limit}")]
    Limit {
        /// Component or operation whose budget was exceeded.
        kind: &'static str,
        /// Observed or requested quantity, in the component's units.
        actual: u64,
        /// Configured maximum, in the same units as `actual`.
        limit: u64,
    },

    /// An article or resource redirect refers to an absent target.
    #[error("redirect target not found: {0}")]
    RedirectMissing(String),

    /// Resolving an article or resource revisited an active redirect.
    #[error("redirect cycle: {0}")]
    RedirectCycle(String),

    /// Redirect traversal exhausted its configured depth limit.
    #[error("redirect traversal limit exceeded: {0}")]
    RedirectLimit(usize),

    /// A source header or compressed block checksum did not match.
    #[error("checksum mismatch")]
    ChecksumMismatch,

    /// A block could not be decompressed with its declared compression format.
    #[error("decompression failed: {0}")]
    DecompressFailed(String),

    /// Encrypted source data could not be decrypted.
    #[error("decryption failed")]
    DecryptFailed,

    /// Text bytes are not valid in the declared encoding.
    #[error("encoding error: {0}")]
    EncodingError(String),

    /// A requested resource key is absent.
    /// An absent article in [`crate::Mdx::lookup`] instead returns an empty vector.
    #[error("key not found: {0}")]
    KeyNotFound(String),

    /// Source or cache structure violates a format or consistency requirement.
    #[error("corrupt dictionary: {0}")]
    Corrupt(String),

    /// Opening a lazily discovered resource volume failed.
    #[error("resource volume {path}: {source}")]
    Resource {
        /// Volume that could not be initialized.
        path: std::path::PathBuf,
        /// Underlying parsing, budget or I/O failure.
        #[source]
        source: Box<Error>,
    },

    /// A filesystem, memory-mapping, cache-locking or cache-publication operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
