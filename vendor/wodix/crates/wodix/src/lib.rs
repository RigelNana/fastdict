//! MDX/MDD parsing and lookup for wodix.
//!
//! This crate exposes read-only dictionaries and immutable record views. It does
//! not render HTML, execute dictionary scripts, or provide a desktop interface.
//! Format decoders and physical block directories are implementation details;
//! supported types and extension traits are exported from the crate root.
//!
//! # Decode and look up entries
//!
//! ```
//! use wodix::{Mdx, Read};
//!
//! let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
//! let dictionary = Mdx::parse(bytes, "example")?;
//! assert_eq!(dictionary.reader().entries().len(), 6);
//! assert_eq!(dictionary.lookup("apples")?, vec!["<p>A fruit.</p>".to_owned()]);
//! # Ok::<(), wodix::Error>(())
//! ```
//!
//! # Raw bytes and record iteration
//!
//! [`Read::read`] and [`Read::records`] return immutable [`Bytes`]. Text decoding
//! via [`Read::text`] removes trailing NUL and whitespace; [`Mdx::lookup`] also
//! follows redirects, deduplicates articles and expands MDX stylesheet markers.
//! These operations intentionally have different semantics.
//!
//! ```
//! use wodix::{Read, Reader};
//!
//! let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
//! let reader = Reader::parse(bytes, "example")?;
//! let mut words = Vec::new();
//! for record in reader.records() {
//!     let (entry, bytes) = record?;
//!     words.push(entry.headword.as_str());
//!     if entry.headword == "apple" {
//!         assert_eq!(bytes.as_ref(), b"<p>A fruit.</p>\0");
//!     }
//! }
//! assert_eq!(words, vec!["apple", "apples", "apricot", "banana", "café", "猫"]);
//! # Ok::<(), wodix::Error>(())
//! ```
//!
//! # Configure component budgets
//!
//! [`Limits`] is forward-compatible: start from its defaults and change the
//! fields needed by the caller. Cache residency excludes views retained by the
//! caller, in-flight decoding and memory-mapped pages; it is not a process RSS cap.
//!
//! ```
//! use wodix::{Limits, Read, Reader};
//!
//! let mut limits = Limits::default();
//! limits.cache = 0;
//! let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
//! let reader = Reader::with_limits(bytes, "example", limits)?;
//! assert_eq!(reader.text(0)?, "<p>A fruit.</p>");
//! assert_eq!(reader.cache_stats().resident, 0);
//! # Ok::<(), wodix::Error>(())
//! ```
//!
//! # Read-only source identity
//!
//! Entry ordinals belong to one source; they are not global or persistent IDs.
//! Metadata and entries cannot be changed behind the index or record directory.
//!
//! ```compile_fail
//! use wodix::{Mdx, Read};
//!
//! let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
//! let dictionary = Mdx::parse(bytes, "example")?;
//! dictionary.reader().entries()[0].headword.clear();
//! # Ok::<(), wodix::Error>(())
//! ```
//!
//! # File safety and side effects
//!
//! Owned or borrowed bytes use the safe parsing constructors. Memory-mapped
//! `open` operations are explicitly unsafe: backing MDX/MDD files must not be
//! modified or truncated while the objects exist. Resource directory paths must
//! also remain stable. Advisory locks alone do not establish these invariants.
//!
//! [`Mdx::open`] may create a derived index cache and lock file and discover MDD
//! volumes. [`Cache`] is available for callers that need explicit cache locations.
//! Cache schema 3 validates canonical keys/postings before rebuilding the FST;
//! loading it is not a zero-work binary FST mapping.
//!
//! # Supported format surface
//!
//! Version 1.x and 2.x layouts, uncompressed/LZO/zlib blocks, supported key-info
//! encryption, UTF-8, UTF-16LE, GBK/GB18030 and Big5 are implemented. Unsupported
//! versions, encodings and encryption modes return typed [`Error`] values. HTML
//! and binary resources remain untrusted application data.

#![warn(missing_docs)]

mod checksum;
mod decode;
mod decompress;
mod decrypt;
mod dict;
mod error;
mod fuzzy;
mod header;
mod index;
mod key_block;
mod limits;
mod link;
mod mdd;
mod number;
mod reader;
mod record_block;
mod stylesheet;
mod types;

pub use dict::{Article, Mdx};
pub use error::Error;
pub use index::{Cache, Index, Lookup, Query};
pub use limits::Limits;
pub use mdd::{Mdd, Resource, ResourceMount, Resources};
pub use reader::{CacheStats, Entries, Read, Reader};
pub use record_block::Bytes;
pub use types::{Encoding, Encryption, Entry, Metadata, StyleSheet, Version};

/// Library result with an overridable error type for adapters and examples.
pub type Result<T, E = Error> = std::result::Result<T, E>;
