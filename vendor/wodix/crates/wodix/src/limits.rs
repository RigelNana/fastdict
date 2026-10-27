//! Per-reader allocation and traversal budgets. These are not a process RSS cap.

use crate::{Error, Result};

/// Independent allocation and traversal budgets used by readers and indexes.
///
/// Byte budgets are measured in bytes, not entries. They constrain individual
/// components or operations, not total process RSS: source storage, allocator
/// overhead, concurrent readers, temporary allocations and [`crate::Bytes`]
/// retained after cache eviction can consume additional memory. Reader metadata,
/// index construction and lookup output each use their own metadata budget.
///
/// Start with [`Default`] and change the fields required by your workload.
/// Zero is a real budget, not an instruction to use an unlimited default.
///
/// ```
/// use wodix::{Limits, Read, Reader};
///
/// let mut limits = Limits::default();
/// limits.cache = 0; // Decode on demand without retaining record blocks.
/// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
/// let reader = Reader::with_limits(data.as_slice(), "fallback", limits)?;
/// assert_eq!(reader.text(0)?, "<p>A fruit.</p>");
/// assert_eq!(reader.cache_stats().resident, 0);
/// # Ok::<(), wodix::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum encoded XML header size in bytes.
    pub header: usize,
    /// Maximum key-block index or serialized index-cache size in bytes.
    /// Also bounds search-query and resource-name input bytes.
    pub key_index: usize,
    /// Maximum compressed size of one key or record block in bytes.
    pub compressed: usize,
    /// Maximum decompressed size of one key or record block in bytes.
    pub block: usize,
    /// Maximum raw record or local resource size in bytes.
    pub record: usize,
    /// Maximum entry count; also bounds work performed by one article lookup.
    pub entries: usize,
    /// Maximum block count in a directory, also used for resource volume discovery.
    pub blocks: usize,
    /// Component-local metadata and working-storage budget in bytes.
    /// Also bounds decoded article work and accumulated style-expanded output.
    pub metadata: usize,
    /// Maximum decompressed bytes retained by one reader's record-block cache.
    /// Zero disables retention; oversized blocks are decoded but not cached.
    /// Caller-owned [`crate::Bytes`] are not charged after eviction.
    pub cache: usize,
    /// Maximum redirect traversal depth for article or resource resolution.
    pub redirects: usize,
    /// Maximum dynamic-programming cells evaluated by one fuzzy query.
    pub fuzzy_cells: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            header: 16 * 1024 * 1024,
            key_index: 64 * 1024 * 1024,
            compressed: 64 * 1024 * 1024,
            block: 64 * 1024 * 1024,
            record: 64 * 1024 * 1024,
            entries: 10_000_000,
            blocks: 1_000_000,
            metadata: 256 * 1024 * 1024,
            cache: 64 * 1024 * 1024,
            redirects: 32,
            fuzzy_cells: 4_000_000,
        }
    }
}

impl Limits {
    /// Check a measured component against its budget, accepting equality.
    ///
    /// `kind` identifies the component in the returned typed error.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Limit`] when `actual` exceeds `limit`.
    pub fn check(kind: &'static str, actual: u64, limit: usize) -> Result<()> {
        if actual > limit as u64 {
            return Err(Error::Limit {
                kind,
                actual,
                limit: limit as u64,
            });
        }
        Ok(())
    }
}
