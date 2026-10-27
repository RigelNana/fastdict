//! Immutable headword lookup and content-bound, atomically published caches.

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt;
use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use unicode_normalization::UnicodeNormalization;

use crate::decode::Cursor;
use crate::fuzzy::Fuzzy;
use crate::types::Entry;
use crate::{Error, Limits, Result};

/// Matching operates on normalized headwords; each visit receives every duplicate ID.
///
/// [`crate::Index`] uses NFC normalization, Unicode lowercasing, then trimming of
/// leading and trailing Unicode whitespace for both stored keys and queries.
#[derive(Clone, Copy, Debug)]
pub enum Query<'a> {
    /// Match every normalized key starting with this normalized prefix.
    Prefix(&'a str),
    /// Match normalized keys within a Unicode-scalar Levenshtein edit distance.
    Fuzzy {
        /// Input headword, normalized before matching.
        word: &'a str,
        /// Maximum insertion, deletion or substitution distance.
        distance: u32,
    },
}

/// Search operations are independent of record decoding and candidate quotas.
///
/// Returned IDs are positions in the exact [`crate::Entry`] slice used to build
/// this index, and must never be applied to another source. Import this trait
/// to call its methods on [`crate::Index`].
pub trait Lookup {
    /// Build an immutable index bound to these source-order entries.
    ///
    /// Duplicate headwords retain every source ID. Implementations must not
    /// substitute IDs from a sorted copy of the directory.
    ///
    /// # Errors
    ///
    /// [`crate::Index`] reports [`crate::Error::Limit`] for entry or metadata
    /// budgets and [`crate::Error::Corrupt`] for index-construction failures.
    fn build(entries: &[Entry], limits: Limits) -> Result<Self>
    where
        Self: Sized;
    /// Return every ID matching a headword, or an empty slice for a miss.
    ///
    /// [`crate::Index`] normalizes the input and returns duplicate IDs in source
    /// order. This infallible operation does not apply query work or input-size
    /// budgets; bound untrusted input at the caller when necessary.
    fn exact(&self, word: &str) -> &[usize];
    /// Visit matching ID groups until exhausted or the visitor returns `false`.
    ///
    /// [`crate::Index`] visits normalized keys in lexical order, not fuzzy-distance
    /// order; each group contains source-order IDs including duplicates. There
    /// is no implicit candidate quota. The visitor may stop after enough results.
    ///
    /// # Errors
    ///
    /// Query size, metadata or fuzzy-work budgets may produce
    /// [`crate::Error::Limit`]. **Visitor calls are not transactional:** an error
    /// may occur after partial output or other visitor side effects, including
    /// during fuzzy traversal. Buffer results and discard them on error if an
    /// all-or-error result is required.
    fn visit<F: FnMut(&[usize]) -> bool>(&self, query: Query<'_>, visitor: F) -> Result<()>;
}

struct Data {
    map: Map<Vec<u8>>,
    postings: Vec<Vec<usize>>,
    fingerprint: [u8; 32],
    limits: Limits,
}

/// Clones share an immutable index; entry IDs retain source order within each group.
///
/// Keys are normalized with NFC, Unicode lowercase and whitespace trimming.
/// This does not strip accents or punctuation. Exact matches retain all duplicate
/// IDs; prefix and fuzzy traversal return normalized keys in lexical order.
/// The index contains no articles and performs no record decompression.
///
/// ```
/// use wodix::{Index, Lookup, Read, Reader};
///
/// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
/// let reader = Reader::parse(data.as_slice(), "fallback")?;
/// let index = Index::build(reader.entries(), reader.limits())?;
/// assert_eq!(index.exact(" APPLE "), &[0]);
/// assert_eq!(reader.text(index.exact("café")[0])?, "<p>A cafe.</p>");
/// assert!(index.exact("missing").is_empty());
/// # Ok::<(), wodix::Error>(())
/// ```
#[derive(Clone)]
pub struct Index {
    data: Arc<Data>,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Index")
            .field("groups", &self.data.postings.len())
            .field("fst_bytes", &self.data.map.as_fst().as_bytes().len())
            .finish_non_exhaustive()
    }
}

impl Index {
    fn fold(word: &str) -> String {
        let normalized: String = word.nfc().collect();
        normalized.to_lowercase().trim().to_owned()
    }

    fn fold_bounded(word: &str, budget: &Budget) -> Result<String> {
        let mut normalized = String::new();
        budget.check(word.len())?;
        normalized.reserve_exact(word.len());
        for ch in word.nfc() {
            budget.check(normalized.len().saturating_add(ch.len_utf8()))?;
            if normalized.len() + ch.len_utf8() > normalized.capacity() {
                normalized.reserve_exact(ch.len_utf8());
            }
            normalized.push(ch);
        }
        let lower_bytes = normalized
            .chars()
            .flat_map(char::to_lowercase)
            .try_fold(0usize, |size, ch| size.checked_add(ch.len_utf8()))
            .ok_or_else(|| Error::Corrupt("normalized headword length overflow".into()))?;
        budget.check(normalized.capacity().saturating_add(lower_bytes))?;
        let mut lower = normalized.to_lowercase();
        let trimmed = lower.trim();
        let start = trimmed.as_ptr() as usize - lower.as_ptr() as usize;
        let end = start + trimmed.len();
        lower.truncate(end);
        lower.drain(..start);
        Ok(lower)
    }

    fn fingerprint(entries: &[Entry]) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        // Preserve the historical domain so rebranding does not invalidate existing caches.
        hash.update(b"woyodict headword index v3\0nfc-lower-trim\0");
        hash.update(&(entries.len() as u64).to_le_bytes());
        for entry in entries {
            hash.update(&(entry.headword.len() as u64).to_le_bytes());
            hash.update(entry.headword.as_bytes());
            hash.update(&entry.record_offset.to_le_bytes());
        }
        *hash.finalize().as_bytes()
    }

    fn scan<A: Automaton, F: FnMut(&[usize]) -> bool>(&self, automaton: A, mut visitor: F) {
        let mut stream = self.data.map.search(automaton).into_stream();
        while let Some((_, posting)) = stream.next() {
            if !visitor(&self.data.postings[posting as usize]) {
                break;
            }
        }
    }

    fn finish(
        builder: MapBuilder<Buffer>,
        postings: Vec<Vec<usize>>,
        fingerprint: [u8; 32],
        limits: Limits,
    ) -> Result<Self> {
        let buffer = builder
            .into_inner()
            .map_err(|error| Error::Corrupt(format!("fst finish: {error}")))?;
        // These bytes were produced locally, never taken from an untrusted cache.
        let map = Map::new(buffer.bytes)
            .map_err(|error| Error::Corrupt(format!("fst construction: {error}")))?;
        Ok(Self {
            data: Arc::new(Data {
                map,
                postings,
                fingerprint,
                limits,
            }),
        })
    }
}

impl Lookup for Index {
    fn build(entries: &[Entry], limits: Limits) -> Result<Self> {
        Limits::check("index entries", entries.len() as u64, limits.entries)?;
        let mut budget = Budget::new(limits.metadata);
        budget.add_array::<(String, usize)>(entries.len())?;
        budget.add_array::<Vec<usize>>(entries.len())?;
        budget.add_array::<usize>(entries.len())?;
        let mut keyed = Vec::with_capacity(entries.len());
        for (id, entry) in entries.iter().enumerate() {
            let key = Self::fold_bounded(&entry.headword, &budget)?;
            budget.add(key.capacity())?;
            keyed.push((key, id));
        }
        keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut builder = MapBuilder::new(Buffer::new(budget.remaining()))
            .map_err(|error| Error::Corrupt(format!("fst builder: {error}")))?;
        let mut postings = Vec::with_capacity(entries.len());
        let mut start = 0;
        while start < keyed.len() {
            let key = &keyed[start].0;
            let mut end = start + 1;
            while end < keyed.len() && keyed[end].0 == *key {
                end += 1;
            }
            builder
                .insert(key.as_bytes(), postings.len() as u64)
                .map_err(|error| Error::Corrupt(format!("fst insert: {error}")))?;
            postings.push(keyed[start..end].iter().map(|(_, id)| *id).collect());
            start = end;
        }
        Self::finish(builder, postings, Self::fingerprint(entries), limits)
    }

    fn exact(&self, word: &str) -> &[usize] {
        match self.data.map.get(Self::fold(word)) {
            Some(id) => &self.data.postings[id as usize],
            None => &[],
        }
    }

    fn visit<F: FnMut(&[usize]) -> bool>(&self, query: Query<'_>, visitor: F) -> Result<()> {
        let word = match query {
            Query::Prefix(word) | Query::Fuzzy { word, .. } => word,
        };
        Limits::check("query bytes", word.len() as u64, self.data.limits.key_index)?;
        let folded = Self::fold_bounded(word, &Budget::new(self.data.limits.metadata))?;
        match query {
            Query::Prefix(_) => {
                self.scan(fst::automaton::Str::new(&folded).starts_with(), visitor);
                Ok(())
            }
            Query::Fuzzy { distance, .. } => {
                let automaton = Fuzzy::new(&folded, distance, self.data.limits)?;
                self.scan(&automaton, visitor);
                automaton.finish()
            }
        }
    }
}

struct Budget {
    limit: usize,
    used: usize,
}

impl Budget {
    fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }

    fn check(&self, bytes: usize) -> Result<()> {
        let total = self.used.checked_add(bytes).ok_or(Error::Limit {
            kind: "index metadata",
            actual: u64::MAX,
            limit: self.limit as u64,
        })?;
        Limits::check("index metadata", total as u64, self.limit)
    }

    fn add(&mut self, bytes: usize) -> Result<()> {
        self.check(bytes)?;
        self.used += bytes;
        Ok(())
    }

    fn add_array<T>(&mut self, count: usize) -> Result<()> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(Error::Limit {
                kind: "index metadata",
                actual: u64::MAX,
                limit: self.limit as u64,
            })?;
        self.add(bytes)
    }

    fn remaining(&self) -> usize {
        self.limit - self.used
    }
}

/// Bounds serialized FST output while the builder produces it, not after allocation.
struct Buffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Buffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(std::io::Error::other(
                "index metadata limit exceeded by FST",
            ));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(self.limit);
            self.bytes.reserve_exact(capacity - self.bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Version 3 stores canonical keys/postings, not unchecked FST node addresses.
struct Stamp {
    entries: u64,
    groups: u64,
    length: u64,
    fingerprint: [u8; 32],
    digest: [u8; 32],
}

impl Stamp {
    const MAGIC: [u8; 8] = *b"MWDICTIX";
    const VERSION: u32 = 3;
    const SIZE: usize = 100;

    fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..8].copy_from_slice(&Self::MAGIC);
        bytes[8..12].copy_from_slice(&Self::VERSION.to_le_bytes());
        bytes[12..20].copy_from_slice(&self.entries.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.groups.to_le_bytes());
        bytes[28..36].copy_from_slice(&self.length.to_le_bytes());
        bytes[36..68].copy_from_slice(&self.fingerprint);
        bytes[68..100].copy_from_slice(&self.digest);
        bytes
    }

    fn decode(bytes: &[u8; Self::SIZE]) -> Result<Option<Self>> {
        if bytes[..8] != Self::MAGIC {
            return Err(Error::Corrupt("index cache magic mismatch".into()));
        }
        if u32::from_le_bytes(bytes[8..12].try_into().unwrap()) != Self::VERSION {
            return Ok(None);
        }
        Ok(Some(Self {
            entries: u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
            groups: u64::from_le_bytes(bytes[20..28].try_into().unwrap()),
            length: u64::from_le_bytes(bytes[28..36].try_into().unwrap()),
            fingerprint: bytes[36..68].try_into().unwrap(),
            digest: bytes[68..100].try_into().unwrap(),
        }))
    }
}

/// A derived cache bound to immutable parsed entries, not source timestamps.
/// Missing/stale caches are misses; explicit load/save report corruption and I/O errors.
/// [`crate::Cache::open`] warns and builds directly if cache I/O or publication is unavailable.
/// The `.lock` sidecar is intentionally persistent to preserve its lock inode.
/// Only validated sorted keys and postings are reused; loading reconstructs the FST
/// and checks normalization against the source rather than mapping cached FST nodes.
///
/// This is a filesystem cache of canonical keys and postings, not the reader's
/// in-memory decompressed-block cache. Loading reads and validates the complete
/// serialized payload, verifies source normalization and reconstructs a fresh
/// FST (finite-state transducer). It is not zero-copy mapping of persisted nodes.
/// Fingerprints bind ordered headwords and record offsets, not record payloads.
/// Cache digests detect damage, but do not authenticate an untrusted publisher.
pub struct Cache<'a> {
    path: PathBuf,
    entries: &'a [Entry],
    limits: Limits,
    fingerprint: [u8; 32],
}

impl std::fmt::Debug for Cache<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Cache")
            .field("entries", &self.entries.len())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<'a> Cache<'a> {
    /// Bind an explicit cache path to an immutable source-order entry directory.
    ///
    /// Computes a content fingerprint but performs no filesystem I/O.
    /// The parent directory must already exist when saving or rebuilding.
    pub fn new(path: impl Into<PathBuf>, entries: &'a [Entry], limits: Limits) -> Self {
        Self {
            path: path.into(),
            entries,
            limits,
            fingerprint: Index::fingerprint(entries),
        }
    }

    /// Load a validated index or build it from the bound source entries.
    ///
    /// Missing, stale or invalid caches are rebuilt. Reads the cache and may
    /// create a persistent `.lock` sidecar, wait for an exclusive advisory lock,
    /// write a temporary file, sync it and atomically replace the cache. The
    /// parent directory is also synced on Unix. The cache is rechecked under the
    /// lock before rebuilding.
    ///
    /// # Errors
    ///
    /// Index-building errors are returned. Cache loading, locking or publication
    /// errors are logged with `tracing` and do not prevent using a freshly built
    /// index. Use [`crate::Cache::load`] or [`crate::Cache::save`] when those
    /// filesystem errors must be reported to the caller.
    pub fn open(&self) -> Result<Index> {
        match self.load() {
            Ok(Some(index)) => return Ok(index),
            Ok(None) => {}
            Err(error) => self.warn(&error),
        }
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(error) => {
                self.warn(&error);
                return Index::build(self.entries, self.limits);
            }
        };
        // Another writer may have completed while this writer waited for its lock.
        match self.load() {
            Ok(Some(index)) => return Ok(index),
            Ok(None) => {}
            Err(error) => self.warn(&error),
        }
        let index = Index::build(self.entries, self.limits)?;
        if let Err(error) = self.publish(&index) {
            self.warn(&error);
        }
        Ok(index)
    }

    /// Atomically replaces the cache after flushing and syncing its temporary file.
    /// Failures before replacement leave the old cache intact. A parent-directory
    /// sync failure can be reported after replacement, with the new file complete.
    ///
    /// Acquires a blocking exclusive advisory lock through a persistent `.lock`
    /// sidecar; creates a temporary file in the cache's parent directory.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Corrupt`] for an index bound to different entries,
    /// [`crate::Error::Limit`] for entry or serialized-size budgets, and
    /// [`crate::Error::Io`] for locking, writing, syncing or replacement failures.
    pub fn save(&self, index: &Index) -> Result<()> {
        self.check_index(index)?;
        let _lock = self.lock()?;
        self.publish(index)
    }

    /// Read, validate and reconstruct an index without writing or locking files.
    ///
    /// Returns `Ok(None)` when absent, from a different cache schema generation,
    /// or bound to different entries. A hit validates the complete
    /// key/posting payload and rebuilds the FST; it does not reuse cached nodes.
    /// The entry slice supplied at construction remains the source of truth.
    ///
    /// # Errors
    ///
    /// Reports malformed caches as [`crate::Error::Corrupt`], exceeded budgets
    /// as [`crate::Error::Limit`], and filesystem or short-read failures as
    /// [`crate::Error::Io`]. A failed load never returns a partial index.
    pub fn load(&self) -> Result<Option<Index>> {
        Limits::check(
            "index entries",
            self.entries.len() as u64,
            self.limits.entries,
        )?;
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let file_length = file.metadata()?.len();
        Limits::check("index cache bytes", file_length, self.limits.key_index)?;
        // Old headers were shorter. Read the schema prefix first so they are clean misses.
        let mut header = [0; Stamp::SIZE];
        file.read_exact(&mut header[..12])?;
        if header[..8] != Stamp::MAGIC {
            return Err(Error::Corrupt("index cache magic mismatch".into()));
        }
        if u32::from_le_bytes(header[8..12].try_into().unwrap()) != Stamp::VERSION {
            return Ok(None);
        }
        file.read_exact(&mut header[12..])?;
        let Some(stamp) = Stamp::decode(&header)? else {
            return Ok(None);
        };
        if stamp.fingerprint != self.fingerprint || stamp.entries != self.entries.len() as u64 {
            return Ok(None);
        }
        Limits::check("index cache groups", stamp.groups, self.entries.len())?;
        Limits::check("index cache payload", stamp.length, self.limits.key_index)?;
        if stamp.length.checked_add(Stamp::SIZE as u64) != Some(file_length) {
            return Err(Error::Corrupt("index cache length mismatch".into()));
        }
        let length = usize::try_from(stamp.length)
            .map_err(|_| Error::Corrupt("index cache length exceeds address space".into()))?;
        let groups = usize::try_from(stamp.groups)
            .map_err(|_| Error::Corrupt("index cache groups exceed address space".into()))?;
        let mut budget = Budget::new(self.limits.metadata);
        budget.add(length)?;
        budget.add_array::<Vec<usize>>(groups)?;
        budget.add_array::<usize>(self.entries.len())?;
        budget.add_array::<bool>(self.entries.len())?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        let mut extra = [0];
        if file.read(&mut extra)? != 0 {
            return Err(Error::Corrupt("index cache has trailing data".into()));
        }
        if blake3::hash(&bytes).as_bytes() != &stamp.digest {
            return Err(Error::Corrupt("index cache payload digest mismatch".into()));
        }
        let mut cursor = Cursor {
            data: &bytes,
            position: 0,
        };
        let mut builder = MapBuilder::new(Buffer::new(budget.remaining()))
            .map_err(|error| Error::Corrupt(format!("fst builder: {error}")))?;
        let mut postings = Vec::with_capacity(groups);
        let mut seen = vec![false; self.entries.len()];
        let mut total = 0usize;
        let mut previous: Option<&str> = None;
        for group_id in 0..groups {
            let key_length = Self::number(&mut cursor)?;
            let key = std::str::from_utf8(cursor.take(key_length)?)
                .map_err(|error| Error::Corrupt(format!("index cache headword: {error}")))?;
            if previous.is_some_and(|previous| previous >= key) {
                return Err(Error::Corrupt(
                    "index cache headwords are not strictly sorted".into(),
                ));
            }
            previous = Some(key);
            let count = Self::number(&mut cursor)?;
            if count == 0
                || count > self.entries.len() - total
                || count > (bytes.len() - cursor.position) / 8
            {
                return Err(Error::Corrupt(
                    "index cache posting count is invalid".into(),
                ));
            }
            let mut group = Vec::with_capacity(count);
            for _ in 0..count {
                let id = Self::number(&mut cursor)?;
                if id >= self.entries.len()
                    || seen[id]
                    || group.last().is_some_and(|previous| *previous >= id)
                {
                    return Err(Error::Corrupt(
                        "index cache posting ID/order is invalid".into(),
                    ));
                }
                let temporary_budget = Budget {
                    limit: budget.limit,
                    used: budget.used + builder.get_ref().bytes.capacity(),
                };
                if Index::fold_bounded(&self.entries[id].headword, &temporary_budget)? != key {
                    return Err(Error::Corrupt(
                        "index cache posting does not match source headword".into(),
                    ));
                }
                seen[id] = true;
                group.push(id);
            }
            total += count;
            builder
                .insert(key, group_id as u64)
                .map_err(|error| Error::Corrupt(format!("fst cache insert: {error}")))?;
            postings.push(group);
        }
        if total != self.entries.len() || cursor.position != bytes.len() {
            return Err(Error::Corrupt(
                "index cache does not cover the source exactly".into(),
            ));
        }
        Ok(Some(Index::finish(
            builder,
            postings,
            self.fingerprint,
            self.limits,
        )?))
    }

    fn number(cursor: &mut Cursor<'_>) -> Result<usize> {
        let value = u64::from_le_bytes(cursor.take(8)?.try_into().unwrap());
        usize::try_from(value)
            .map_err(|_| Error::Corrupt("index cache integer exceeds address space".into()))
    }

    fn warn(&self, error: &Error) {
        tracing::warn!(%error, path = %self.path.display(), "index cache unavailable; using parsed entries");
    }

    fn parent(&self) -> &Path {
        self.path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }

    fn lock(&self) -> Result<File> {
        let mut path = self.path.as_os_str().to_os_string();
        path.push(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    fn check_index(&self, index: &Index) -> Result<()> {
        if index.data.fingerprint != self.fingerprint {
            return Err(Error::Corrupt(
                "cannot save an index built from different entries".into(),
            ));
        }
        Limits::check(
            "index entries",
            self.entries.len() as u64,
            self.limits.entries,
        )
    }

    fn payload<F: FnMut(&[u8]) -> Result<()>>(index: &Index, mut write: F) -> Result<()> {
        let mut stream = index.data.map.stream();
        while let Some((key, posting)) = stream.next() {
            write(&(key.len() as u64).to_le_bytes())?;
            write(key)?;
            let group = &index.data.postings[posting as usize];
            write(&(group.len() as u64).to_le_bytes())?;
            for &id in group {
                write(&(id as u64).to_le_bytes())?;
            }
        }
        Ok(())
    }

    fn publish(&self, index: &Index) -> Result<()> {
        self.check_index(index)?;
        let mut length = 0u64;
        let mut hash = blake3::Hasher::new();
        Self::payload(index, |bytes| {
            length = length
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| Error::Corrupt("index cache payload length overflow".into()))?;
            let file_length = length
                .checked_add(Stamp::SIZE as u64)
                .ok_or_else(|| Error::Corrupt("index cache file length overflow".into()))?;
            Limits::check("index cache bytes", file_length, self.limits.key_index)?;
            hash.update(bytes);
            Ok(())
        })?;
        Limits::check(
            "index cache bytes",
            length + Stamp::SIZE as u64,
            self.limits.key_index,
        )?;
        let stamp = Stamp {
            entries: self.entries.len() as u64,
            groups: index.data.postings.len() as u64,
            length,
            fingerprint: self.fingerprint,
            digest: *hash.finalize().as_bytes(),
        };
        let mut pending = tempfile::NamedTempFile::new_in(self.parent())?;
        {
            let mut writer = std::io::BufWriter::new(pending.as_file_mut());
            writer.write_all(&stamp.encode())?;
            Self::payload(index, |bytes| {
                writer.write_all(bytes)?;
                Ok(())
            })?;
            writer.flush()?;
        }
        pending.as_file().sync_all()?;
        // keep clears Windows' temporary attribute; the new guard still cleans up
        // failed replacements. std::fs::rename supports replacing open Windows
        // files, unlike tempfile's MoveFileExW-only persist implementation.
        let (file, path) = pending.keep().map_err(|error| Error::Io(error.error))?;
        drop(file);
        let mut pending = tempfile::TempPath::try_from_path(path)?;
        std::fs::rename(&pending, &self.path)?;
        pending.disable_cleanup(true);
        #[cfg(unix)]
        File::open(self.parent())?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::sync::Barrier;

    struct Fixture;

    impl Fixture {
        fn entries(words: &[&str]) -> Vec<Entry> {
            words
                .iter()
                .enumerate()
                .map(|(id, word)| Entry {
                    record_offset: (id * 100) as u64,
                    headword: (*word).to_owned(),
                })
                .collect()
        }

        fn collect(index: &Index, query: Query<'_>) -> Vec<usize> {
            let mut ids = Vec::new();
            index
                .visit(query, |group| {
                    ids.extend_from_slice(group);
                    true
                })
                .unwrap();
            ids
        }

        fn rewrite(path: &Path, mutate: impl FnOnce(&mut Vec<u8>)) {
            let mut bytes = fs::read(path).unwrap();
            mutate(&mut bytes);
            let digest = *blake3::hash(&bytes[Stamp::SIZE..]).as_bytes();
            bytes[68..100].copy_from_slice(&digest);
            fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn normalized_lookup_retains_duplicates_in_source_order() {
        let entries = Fixture::entries(&["Apple", "BANANA", "é", "e\u{0301}", "apple"]);
        let index = Index::build(&entries, Limits::default()).unwrap();
        assert_eq!(index.exact(" APPLE "), &[0, 4]);
        assert_eq!(index.exact("Banana"), &[1]);
        assert_eq!(index.exact(" e\u{0301} "), &[2, 3]);
        assert_eq!(index.exact("missing"), &[]);
    }

    #[test]
    fn visitors_control_unique_headword_quotas_without_losing_groups() {
        let entries =
            Fixture::entries(&["app", "app", "app", "App", "apple", "application", "banana"]);
        let index = Index::build(&entries, Limits::default()).unwrap();
        let mut seen = HashSet::new();
        let mut selected = Vec::new();
        index
            .visit(Query::Prefix("APP"), |group| {
                for &id in group {
                    if seen.insert(&entries[id].headword) {
                        selected.push(id);
                        if selected.len() == 3 {
                            return false;
                        }
                    }
                }
                true
            })
            .unwrap();
        assert_eq!(selected, vec![0, 3, 4]);
        assert_eq!(
            Fixture::collect(&index, Query::Prefix("app")),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(
            Fixture::collect(&index, Query::Prefix("xyz")),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn fuzzy_distance_and_empty_queries_have_explicit_boundaries() {
        let entries = Fixture::entries(&["hello", "hallo", "world", "help", "hello"]);
        let index = Index::build(&entries, Limits::default()).unwrap();
        assert_eq!(
            Fixture::collect(
                &index,
                Query::Fuzzy {
                    word: "hello",
                    distance: 1
                }
            ),
            vec![1, 0, 4]
        );
        assert_eq!(
            Fixture::collect(
                &index,
                Query::Fuzzy {
                    word: "hello",
                    distance: 0
                }
            ),
            vec![0, 4]
        );
        assert_eq!(
            Fixture::collect(
                &index,
                Query::Fuzzy {
                    word: "zzzzz",
                    distance: 1
                }
            ),
            Vec::<usize>::new()
        );
        let mut first = Vec::new();
        index
            .visit(
                Query::Fuzzy {
                    word: "hello",
                    distance: 1,
                },
                |group| {
                    first.extend_from_slice(group);
                    false
                },
            )
            .unwrap();
        assert_eq!(first, vec![1]);
        let unicode =
            Index::build(&Fixture::entries(&["猫", "狗", "猫咪"]), Limits::default()).unwrap();
        assert_eq!(
            Fixture::collect(
                &unicode,
                Query::Fuzzy {
                    word: "猫",
                    distance: 1
                }
            ),
            vec![1, 0, 2]
        );
        let empty = Index::build(&[], Limits::default()).unwrap();
        assert_eq!(empty.exact(""), &[]);
        assert_eq!(
            Fixture::collect(&empty, Query::Prefix("")),
            Vec::<usize>::new()
        );
        assert_eq!(
            Fixture::collect(
                &empty,
                Query::Fuzzy {
                    word: "word",
                    distance: 1
                }
            ),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn roundtrip_preserves_lookup_and_rejects_same_count_source_replacements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        let entries = Fixture::entries(&["alpha", "beta", "alpha", "é"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        cache
            .save(&Index::build(&entries, Limits::default()).unwrap())
            .unwrap();
        let loaded = cache.load().unwrap().unwrap();
        assert_eq!(loaded.exact("alpha"), &[0, 2]);
        assert_eq!(
            Fixture::collect(&loaded, Query::Prefix("")),
            vec![0, 2, 1, 3]
        );
        let changed = Fixture::entries(&["delta", "beta", "delta", "é"]);
        let replacement = Cache::new(&path, &changed, Limits::default());
        assert!(replacement.load().unwrap().is_none());
        let rebuilt = replacement.open().unwrap();
        assert_eq!(rebuilt.exact("alpha"), &[]);
        assert_eq!(rebuilt.exact("delta"), &[0, 2]);
        assert!(cache.load().unwrap().is_none());
        let mut moved = changed.clone();
        moved[0].record_offset += 1;
        assert!(
            Cache::new(&path, &moved, Limits::default())
                .load()
                .unwrap()
                .is_none()
        );
        let mut reordered = changed.clone();
        reordered.swap(0, 1);
        assert!(
            Cache::new(&path, &reordered, Limits::default())
                .load()
                .unwrap()
                .is_none()
        );
        let spellings = Fixture::entries(&["DELTA", "beta", "delta", "é"]);
        assert!(
            Cache::new(&path, &spellings, Limits::default())
                .load()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn failed_save_preserves_previous_complete_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        let entries = Fixture::entries(&["old", "old"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        let index = cache.open().unwrap();
        let changed = Fixture::entries(&["new", "new"]);
        let foreign = Index::build(&changed, Limits::default()).unwrap();
        assert!(cache.save(&foreign).is_err());
        let limited = Cache::new(
            &path,
            &changed,
            Limits {
                key_index: Stamp::SIZE,
                ..Limits::default()
            },
        );
        assert!(limited.save(&foreign).is_err());
        assert_eq!(cache.load().unwrap().unwrap().exact("old"), &[0, 1]);
        assert_eq!(index.exact("old"), &[0, 1]);
    }

    #[test]
    fn failed_replacement_preserves_destination_and_removes_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("sentinel"), b"previous").unwrap();
        let entries = Fixture::entries(&["word"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        let index = Index::build(&entries, Limits::default()).unwrap();

        assert!(matches!(cache.save(&index), Err(Error::Io(_))));
        assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"previous");
        let remaining: HashSet<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            remaining,
            HashSet::from([
                std::ffi::OsString::from("words.idx"),
                std::ffi::OsString::from("words.idx.lock"),
            ])
        );
    }

    #[test]
    fn malformed_payloads_cannot_forge_valid_lookup_mappings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        let entries = Fixture::entries(&["alpha", "beta"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        let index = Index::build(&entries, Limits::default()).unwrap();
        cache.save(&index).unwrap();
        Fixture::rewrite(&path, |bytes| {
            bytes[Stamp::SIZE + 8] = b'z';
        });
        assert!(cache.load().is_err());
        cache.save(&index).unwrap();
        Fixture::rewrite(&path, |bytes| {
            let end = bytes.len();
            bytes[end - 8..].copy_from_slice(&0u64.to_le_bytes());
        });
        assert!(cache.load().is_err());
        cache.save(&index).unwrap();
        Fixture::rewrite(&path, |bytes| {
            let end = bytes.len();
            bytes[end - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
        });
        assert!(cache.load().is_err());
        cache.save(&index).unwrap();
        Fixture::rewrite(&path, |bytes| {
            bytes[Stamp::SIZE..Stamp::SIZE + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        });
        assert!(cache.load().is_err());
        assert_eq!(cache.open().unwrap().exact("beta"), &[1]);
    }

    #[test]
    fn cached_duplicate_order_and_empty_coverage_are_verified() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("duplicates.idx");
        let entries = Fixture::entries(&["word", "word"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        cache.open().unwrap();
        Fixture::rewrite(&path, |bytes| {
            let end = bytes.len();
            bytes[end - 16..end - 8].copy_from_slice(&1u64.to_le_bytes());
            bytes[end - 8..].copy_from_slice(&0u64.to_le_bytes());
        });
        assert!(cache.load().is_err());
        assert_eq!(cache.open().unwrap().exact("word"), &[0, 1]);
        let empty = Cache::new(directory.path().join("empty.idx"), &[], Limits::default());
        empty.open().unwrap();
        let loaded = empty.load().unwrap().unwrap();
        assert_eq!(loaded.exact("word"), &[]);
        assert_eq!(
            Fixture::collect(&loaded, Query::Prefix("")),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn missing_legacy_truncated_and_oversized_caches_are_handled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        let entries = Fixture::entries(&["word"]);
        let cache = Cache::new(&path, &entries, Limits::default());
        assert!(cache.load().unwrap().is_none());
        let mut legacy = vec![0; 36];
        legacy[..8].copy_from_slice(&Stamp::MAGIC);
        legacy[8..12].copy_from_slice(&2u32.to_le_bytes());
        fs::write(&path, legacy).unwrap();
        assert!(cache.load().unwrap().is_none());
        let index = cache.open().unwrap();
        let complete = fs::read(&path).unwrap();
        fs::write(&path, &complete[..complete.len() - 1]).unwrap();
        assert!(cache.load().is_err());
        cache.save(&index).unwrap();
        let mut corrupted = complete.clone();
        let end = corrupted.len();
        corrupted[end - 1] ^= 1;
        fs::write(&path, corrupted).unwrap();
        assert!(cache.load().is_err());
        fs::write(&path, &complete).unwrap();
        let small = Cache::new(
            &path,
            &entries,
            Limits {
                key_index: complete.len() - 1,
                ..Limits::default()
            },
        );
        assert!(matches!(small.load(), Err(Error::Limit { .. })));
        assert_eq!(small.open().unwrap().exact("word"), &[0]);
        Fixture::rewrite(&path, |bytes| {
            bytes[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
        });
        assert!(matches!(cache.load(), Err(Error::Limit { .. })));
    }

    #[test]
    fn unavailable_cache_storage_does_not_hide_source_build_errors() {
        let directory = tempfile::tempdir().unwrap();
        let entries = Fixture::entries(&["word"]);
        let cache = Cache::new(
            directory.path().join("missing/words.idx"),
            &entries,
            Limits::default(),
        );
        assert_eq!(cache.open().unwrap().exact("word"), &[0]);
        assert!(
            cache
                .save(&Index::build(&entries, Limits::default()).unwrap())
                .is_err()
        );
        let limited = Cache::new(
            &cache.path,
            &entries,
            Limits {
                entries: 0,
                ..Limits::default()
            },
        );
        assert!(matches!(limited.open(), Err(Error::Limit { .. })));
        assert!(matches!(
            Index::build(
                &entries,
                Limits {
                    metadata: 1,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit { .. })
        ));
    }

    #[test]
    fn replacing_an_open_cache_preserves_the_existing_reader_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.idx");
        let old = Fixture::entries(&["alpha", "alpha", "beta"]);
        let new = Fixture::entries(&["gamma", "delta", "gamma"]);
        let old_cache = Cache::new(&path, &old, Limits::default());
        let new_cache = Cache::new(&path, &new, Limits::default());
        old_cache
            .save(&Index::build(&old, Limits::default()).unwrap())
            .unwrap();
        let original = fs::read(&path).unwrap();
        let mut snapshot = File::open(&path).unwrap();

        new_cache
            .save(&Index::build(&new, Limits::default()).unwrap())
            .unwrap();
        let published = new_cache.load().unwrap().unwrap();
        assert_eq!(published.exact("gamma"), &[0, 2]);
        assert_eq!(published.exact("alpha"), &[]);
        assert!(old_cache.load().unwrap().is_none());

        let mut retained = Vec::new();
        snapshot.read_to_end(&mut retained).unwrap();
        assert_eq!(retained, original);
    }

    #[test]
    fn process_writers_share_the_same_atomic_cache_protocol() {
        const CHILD_PATH: &str = "WODIX_INDEX_PROCESS_TEST_PATH";
        const CHILD_SOURCE: &str = "WODIX_INDEX_PROCESS_TEST_SOURCE";
        let old = Fixture::entries(&["alpha", "alpha", "beta"]);
        let new = Fixture::entries(&["gamma", "delta", "gamma"]);
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let entries = if std::env::var(CHILD_SOURCE).unwrap() == "old" {
                &old
            } else {
                &new
            };
            let cache = Cache::new(PathBuf::from(path), entries, Limits::default());
            let expected = Index::build(entries, Limits::default()).unwrap();
            for _ in 0..12 {
                cache.save(&expected).unwrap();
                let opened = cache.open().unwrap();
                assert_eq!(
                    Fixture::collect(&opened, Query::Prefix("")),
                    Fixture::collect(&expected, Query::Prefix(""))
                );
            }
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("process.idx");
        let old_cache = Cache::new(&path, &old, Limits::default());
        let new_cache = Cache::new(&path, &new, Limits::default());
        old_cache.open().unwrap();
        let mut children = Vec::new();
        for source in ["old", "new"] {
            children.push(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("index::tests::process_writers_share_the_same_atomic_cache_protocol")
                    .arg("--exact")
                    .env(CHILD_PATH, &path)
                    .env(CHILD_SOURCE, source)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for _ in 0..100 {
            if let Some(index) = old_cache.load().unwrap() {
                assert_eq!(index.exact("alpha"), &[0, 1]);
                assert_eq!(index.exact("gamma"), &[]);
            }
            if let Some(index) = new_cache.load().unwrap() {
                assert_eq!(index.exact("gamma"), &[0, 2]);
                assert_eq!(index.exact("alpha"), &[]);
            }
        }
        let results: Vec<_> = children
            .into_iter()
            .map(|child| child.wait_with_output().unwrap())
            .collect();
        for result in results {
            assert!(
                result.status.success(),
                "cache writer {}:\n{}\n{}",
                result.status,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert!(old_cache.load().unwrap().is_some() ^ new_cache.load().unwrap().is_some());
    }

    #[test]
    fn concurrent_writers_and_readers_observe_only_complete_source_bound_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("words.idx");
        let old = Fixture::entries(&["alpha", "alpha", "beta"]);
        let new = Fixture::entries(&["gamma", "delta", "gamma"]);
        Cache::new(&path, &old, Limits::default()).open().unwrap();
        let barrier = Barrier::new(4);
        std::thread::scope(|scope| {
            for entries in [&old, &new] {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let cache = Cache::new(path, entries, Limits::default());
                    let index = Index::build(entries, Limits::default()).unwrap();
                    barrier.wait();
                    for _ in 0..20 {
                        cache.save(&index).unwrap();
                        let opened = cache.open().unwrap();
                        assert_eq!(
                            opened.exact(&entries[0].headword),
                            index.exact(&entries[0].headword)
                        );
                    }
                });
            }
            for entries in [&old, &new] {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let cache = Cache::new(path, entries, Limits::default());
                    let expected = Index::build(entries, Limits::default()).unwrap();
                    barrier.wait();
                    for _ in 0..100 {
                        if let Some(loaded) = cache.load().unwrap() {
                            assert_eq!(
                                Fixture::collect(&loaded, Query::Prefix("")),
                                Fixture::collect(&expected, Query::Prefix(""))
                            );
                            assert_eq!(
                                loaded.exact(&entries[0].headword),
                                expected.exact(&entries[0].headword)
                            );
                        }
                    }
                });
            }
        });
        let old_loaded = Cache::new(&path, &old, Limits::default()).load().unwrap();
        let new_loaded = Cache::new(&path, &new, Limits::default()).load().unwrap();
        assert!(old_loaded.is_some() ^ new_loaded.is_some());
    }
}
