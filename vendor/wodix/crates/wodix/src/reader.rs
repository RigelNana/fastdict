//! One binary reader for MDX articles and MDD resources.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lru::LruCache;
use memmap2::Mmap;

use crate::decode::{Cursor, Decode};
use crate::decompress::Block;
use crate::header::Header;
use crate::key_block::{KeyHeader, Keys};
use crate::record_block::{Bytes, RecordHeader, Records};
use crate::types::{Entry, Locator, Metadata, Size, Span};
use crate::{Error, Limits, Result};

/// Read-only dictionary data, independent of source ownership and indexing.
///
/// Entry IDs index [`crate::Read::entries`] in original source order. Never
/// apply IDs from another reader or a differently constructed index.
/// Implementors must keep this directory and the corresponding records stable.
pub trait Read {
    /// Return decoded source metadata without reading record payloads.
    fn metadata(&self) -> &Metadata;
    /// Return the source-order directory, retaining aliases and duplicate keys.
    /// Slice positions are the IDs accepted by [`crate::Read::read`].
    fn entries(&self) -> &[Entry];
    /// Return this source's component-local budgets.
    fn limits(&self) -> Limits;
    /// Read one entry's raw, decompressed bytes without text transformations.
    ///
    /// MDX terminators and redirect markers remain intact; MDD data is binary.
    /// A single-block record shares immutable backing storage. A record spanning
    /// blocks is assembled into owned storage. Reading may populate the cache.
    ///
    /// # Errors
    ///
    /// [`crate::Reader`] reports [`crate::Error::Corrupt`] for an invalid ID or
    /// record directory, [`crate::Error::Limit`] for exceeded budgets, and typed
    /// checksum or decompression errors for damaged record blocks.
    fn read(&self, id: usize) -> Result<Bytes>;

    /// Decode a raw record with the source encoding and trim its ending.
    ///
    /// Removes trailing NUL characters first, then trailing Unicode whitespace.
    /// Leading whitespace remains. Redirect markers and style markers are not
    /// interpreted; use [`crate::Mdx::lookup`] for resolved, expanded articles.
    /// Binary MDD resources should normally be accessed with [`crate::Read::read`].
    ///
    /// # Errors
    ///
    /// Propagates read errors and returns [`crate::Error::EncodingError`] when
    /// the record is invalid in the declared encoding.
    fn text(&self, id: usize) -> Result<String> {
        let bytes = self.read(id)?;
        let mut text = self
            .metadata()
            .encoding
            .decode(bytes.as_ref())?
            .into_owned();
        let len = text.trim_end_matches('\0').trim_end().len();
        text.truncate(len);
        Ok(text)
    }

    /// Iterate over directory entries and raw bytes in original source order.
    ///
    /// An error advances past that entry, so calling `next` again attempts the
    /// following record rather than retrying the failed one. Collecting into a
    /// `Result<Vec<_>>` stops at the first error; manual iteration can continue.
    ///
    /// ```
    /// use wodix::{Read, Reader};
    ///
    /// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
    /// let reader = Reader::parse(data.as_slice(), "fallback")?;
    /// let records = reader.records().collect::<wodix::Result<Vec<_>>>()?;
    /// assert_eq!(records.len(), 6);
    /// assert_eq!(records[0].0.headword, "apple");
    /// assert_eq!(records[0].1.as_ref(), b"<p>A fruit.</p>\0");
    /// # Ok::<(), wodix::Error>(())
    /// ```
    fn records(&self) -> Entries<'_, Self>
    where
        Self: Sized,
    {
        Entries {
            reader: self,
            position: 0,
        }
    }
}

/// Original key order, including aliases and duplicate headwords.
/// A failed record advances the iterator just like a successful record.
///
/// Created by [`crate::Read::records`]. Each item pairs borrowed directory
/// metadata with a raw [`crate::Bytes`] view; no redirects or styles are applied.
pub struct Entries<'a, R: Read + ?Sized> {
    reader: &'a R,
    position: usize,
}

impl<'a, R: Read + ?Sized> Iterator for Entries<'a, R> {
    type Item = Result<(&'a Entry, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.position;
        let entry = self.reader.entries().get(id)?;
        self.position += 1;
        Some(self.reader.read(id).map(|bytes| (entry, bytes)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.reader.entries().len().saturating_sub(self.position);
        (remaining, Some(remaining))
    }
}

impl<R: Read + ?Sized> std::fmt::Debug for Entries<'_, R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Entries")
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

/// Snapshot of record-block cache ownership and access counters.
///
/// Outstanding [`crate::Bytes`] may keep evicted blocks alive; this is not an
/// accounting of all decoded data or process memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    /// Block accesses served from retained decompressed storage, saturating at `u64::MAX`.
    pub hits: u64,
    /// Block accesses not found in the cache, including attempts that fail decoding.
    /// The counter saturates at `u64::MAX`.
    pub misses: u64,
    /// Decompressed bytes currently owned by the cache.
    pub resident: usize,
    /// Number of decompressed blocks currently retained.
    pub blocks: usize,
}

#[derive(Debug)]
struct BlockCache {
    blocks: LruCache<usize, Arc<[u8]>>,
    hits: u64,
    misses: u64,
    resident: usize,
}

impl BlockCache {
    fn new() -> Self {
        Self {
            blocks: LruCache::unbounded(),
            hits: 0,
            misses: 0,
            resident: 0,
        }
    }

    fn make_room(&mut self, size: usize, budget: usize) {
        while self.resident > budget - size {
            if let Some((_, block)) = self.blocks.pop_lru() {
                self.resident -= block.len();
            } else {
                break;
            }
        }
    }
}

/// Binary MDX/MDD reader backed by any stable byte container.
///
/// Slices, owned vectors and memory maps share the same parser. Construction
/// decodes keys and validates directories; record payloads are decoded lazily.
/// This reader does not create index-cache files or discover resource volumes.
/// Its `Debug` output contains only counts and format details, never source
/// bytes, metadata strings or cached payloads.
pub struct Reader<B> {
    data: B,
    info: Metadata,
    entries: Vec<Entry>,
    key_blocks: usize,
    blocks: Vec<Span>,
    records: Vec<Locator>,
    offset: u64,
    limits: Limits,
    cache: Mutex<BlockCache>,
}

impl<B> std::fmt::Debug for Reader<B> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Reader")
            .field("version", &self.info.version)
            .field("encoding", &self.info.encoding)
            .field("entries", &self.entries.len())
            .field("blocks", &self.blocks.len())
            .finish_non_exhaustive()
    }
}

impl<B: AsRef<[u8]>> Reader<B> {
    /// Parse borrowed or owned source bytes with default [`crate::Limits`].
    ///
    /// `fallback` is a title used when the source omits a usable title; it is
    /// not an encoding hint. No filesystem operations are performed.
    ///
    /// # Errors
    ///
    /// Returns typed header, unsupported-format, encoding, checksum,
    /// decompression, corruption or budget errors. Successful parsing does not
    /// imply that every lazily decoded record is valid.
    ///
    /// ```
    /// use wodix::{Read, Reader};
    ///
    /// let data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/basic.mdx"));
    /// let reader = Reader::parse(data.as_slice(), "fallback")?;
    /// assert_eq!(reader.metadata().title, "Offline fixture");
    /// assert_eq!(reader.entries().len(), 6);
    /// assert_eq!(reader.read(0)?.as_ref(), b"<p>A fruit.</p>\0");
    /// assert_eq!(reader.text(0)?, "<p>A fruit.</p>");
    /// assert_eq!(reader.text(1)?, "@@@LINK=apple");
    /// # Ok::<(), wodix::Error>(())
    /// ```
    pub fn parse(data: B, fallback: &str) -> Result<Self> {
        Self::with_limits(data, fallback, Limits::default())
    }

    /// Parse source bytes with explicit component budgets and a fallback title.
    ///
    /// See [`crate::Reader::parse`] for lazy validation and source ownership.
    /// Budgets are not a process-memory cap; see [`crate::Limits`].
    ///
    /// # Errors
    ///
    /// Returns the same typed parsing errors as [`crate::Reader::parse`],
    /// including [`crate::Error::Limit`] when a configured budget is exceeded.
    pub fn with_limits(data: B, fallback: &str, limits: Limits) -> Result<Self> {
        let mut cursor = Cursor {
            data: data.as_ref(),
            position: 0,
        };
        let info = cursor.read(&Header {
            fallback,
            limits: &limits,
        })?;
        let mut metadata = 0usize;
        Self::charge(&mut metadata, info.title.capacity(), limits)?;
        Self::charge(&mut metadata, info.description.capacity(), limits)?;
        for (key, value) in &info.attributes {
            Self::charge_array::<(String, String)>(&mut metadata, 1, limits)?;
            Self::charge(&mut metadata, key.capacity(), limits)?;
            Self::charge(&mut metadata, value.capacity(), limits)?;
        }
        for (open, close) in info.stylesheets.values() {
            Self::charge(
                &mut metadata,
                std::mem::size_of::<(i32, (String, String))>(),
                limits,
            )?;
            Self::charge(&mut metadata, open.capacity(), limits)?;
            Self::charge(&mut metadata, close.capacity(), limits)?;
        }
        let keys = cursor.read(&KeyHeader {
            version: info.version,
        })?;
        Limits::check("key blocks", keys.num_blocks, limits.blocks)?;
        Limits::check("entries", keys.num_entries, limits.entries)?;
        Limits::check("key index", keys.info_compressed_size, limits.key_index)?;
        let count = Self::address(keys.num_entries)?;
        let key_blocks = Self::address(keys.num_blocks)?;
        Self::charge_array::<Entry>(&mut metadata, count, limits)?;
        Self::charge_array::<Locator>(&mut metadata, count, limits)?;
        Self::charge_array::<Size>(&mut metadata, key_blocks, limits)?;
        let index_limits = Limits {
            blocks: key_blocks,
            entries: count,
            metadata: limits.metadata - metadata + key_blocks * std::mem::size_of::<Size>(),
            ..limits
        };
        let parser = Keys {
            version: info.version,
            encoding: &info.encoding,
            encrypted: info.encrypted,
            limits: &index_limits,
        };
        let bytes = cursor.take(Self::address(keys.info_compressed_size)?)?;
        let sizes = parser.info(bytes, keys.info_decompressed_size, keys.num_entries)?;
        if sizes.len() != key_blocks {
            return Err(Error::Corrupt(
                "key block count disagrees with directory".into(),
            ));
        }
        if sizes.capacity() > key_blocks {
            Self::charge_array::<Size>(&mut metadata, sizes.capacity() - key_blocks, limits)?;
        }
        let key_bytes = sizes.iter().try_fold(0u64, |total, size| {
            total
                .checked_add(size.compressed)
                .ok_or_else(|| Error::Corrupt("key block sizes overflow".into()))
        })?;
        if key_bytes != keys.blocks_total_size {
            return Err(Error::Corrupt(
                "key block sizes disagree with header".into(),
            ));
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|error| Error::Corrupt(format!("key directory allocation: {error}")))?;
        if entries.capacity() > count {
            Self::charge_array::<Entry>(&mut metadata, entries.capacity() - count, limits)?;
        }
        for size in &sizes {
            Limits::check("compressed block", size.compressed, limits.compressed)?;
            Limits::check("decompressed block", size.decompressed, limits.block)?;
            let bytes = cursor.take(Self::address(size.compressed)?)?;
            let decoded_size = Self::address(size.decompressed)?;
            let block = Block {
                size: decoded_size,
                limit: decoded_size,
            };
            let (decoded, _) = block.decode(bytes)?;
            if decoded.len() as u64 != size.decompressed {
                return Err(Error::Corrupt(
                    "key block size disagrees with directory".into(),
                ));
            }
            let remaining = Limits {
                entries: count - entries.len(),
                metadata: limits.metadata - metadata,
                ..limits
            };
            let parser = Keys {
                limits: &remaining,
                ..parser
            };
            let batch = parser.entries(&decoded)?;
            if batch.len() > count - entries.len() {
                return Err(Error::Corrupt("key entry count exceeds header".into()));
            }
            for entry in &batch {
                Self::charge(&mut metadata, entry.headword.capacity(), limits)?;
            }
            entries.extend(batch);
        }
        if entries.len() != count {
            return Err(Error::Corrupt(
                "key entry count disagrees with header".into(),
            ));
        }
        metadata -= sizes.capacity() * std::mem::size_of::<Size>();
        drop(sizes);

        let records = cursor.read(&RecordHeader {
            version: info.version,
        })?;
        Limits::check("record blocks", records.num_record_blocks, limits.blocks)?;
        Limits::check("entries", records.num_entries, limits.entries)?;
        if records.num_entries != keys.num_entries {
            return Err(Error::Corrupt(
                "record and key entry counts disagree".into(),
            ));
        }
        let record_blocks = Self::address(records.num_record_blocks)?;
        Self::charge_array::<Span>(&mut metadata, record_blocks, limits)?;
        let directory_size = record_blocks
            .checked_mul(info.version.number_size() * 2)
            .ok_or_else(|| Error::Corrupt("record directory size overflow".into()))?;
        if records.record_info_size != directory_size as u64 {
            return Err(Error::Corrupt(
                "record directory size disagrees with header".into(),
            ));
        }
        let bytes = cursor.take(directory_size)?;
        let blocks = Records::index(bytes, records.num_record_blocks, info.version, &limits)?;
        if blocks.capacity() > record_blocks {
            Self::charge_array::<Span>(&mut metadata, blocks.capacity() - record_blocks, limits)?;
        }
        let offset = cursor.position as u64;
        if blocks.last().map_or(0, |last| last.end_pos) != records.total_records_size {
            return Err(Error::Corrupt(
                "record block sizes disagree with header".into(),
            ));
        }
        // Taking a borrowed range checks file extent; it does not read payload pages.
        cursor.take(Self::address(records.total_records_size)?)?;
        let locations = Records {
            blocks: &blocks,
            offset,
        }
        .locate(&entries, &limits)?;
        if locations.capacity() > count {
            Self::charge_array::<Locator>(&mut metadata, locations.capacity() - count, limits)?;
        }
        Ok(Self {
            data,
            info,
            entries,
            key_blocks,
            blocks,
            records: locations,
            offset,
            limits,
            cache: Mutex::new(BlockCache::new()),
        })
    }

    /// Number of compressed key blocks in the source directory.
    pub fn key_block_count(&self) -> usize {
        self.key_blocks
    }

    /// Number of compressed record blocks in the source directory.
    pub fn record_block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Logical byte length of a record, without decoding its payload.
    ///
    /// Returns a corruption error for an ID outside this reader's directory.
    pub fn record_size(&self, id: usize) -> Result<u64> {
        self.records
            .get(id)
            .map(|record| record.record_size)
            .ok_or_else(|| Error::Corrupt(format!("entry index {id} out of range")))
    }

    fn address(value: u64) -> Result<usize> {
        usize::try_from(value)
            .map_err(|_| Error::Corrupt("size or offset exceeds address space".into()))
    }

    fn charge(used: &mut usize, amount: usize, limits: Limits) -> Result<()> {
        let total = used
            .checked_add(amount)
            .ok_or_else(|| Error::Corrupt("metadata allocation size overflow".into()))?;
        Limits::check("metadata", total as u64, limits.metadata)?;
        *used = total;
        Ok(())
    }

    fn charge_array<T>(used: &mut usize, count: usize, limits: Limits) -> Result<()> {
        let amount = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| Error::Corrupt("metadata allocation size overflow".into()))?;
        Self::charge(used, amount, limits)
    }

    /// Cache statistics do not include views retained by callers after eviction.
    ///
    /// Takes the cache mutex to read counters, but does not decode records.
    /// A cache miss is a block access, not necessarily an entry access or a
    /// successful decompression.
    pub fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        CacheStats {
            hits: cache.hits,
            misses: cache.misses,
            resident: cache.resident,
            blocks: cache.blocks.len(),
        }
    }

    fn block(&self, id: usize) -> Result<Arc<[u8]>> {
        // Holding the lock through decoding coalesces concurrent cacheable reads.
        // A zero budget or an oversized block deliberately retains no decoded data.
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(block) = cache.blocks.get(&id).cloned() {
            cache.hits = cache.hits.saturating_add(1);
            return Ok(block);
        }
        cache.misses = cache.misses.saturating_add(1);
        let block = self
            .blocks
            .get(id)
            .ok_or_else(|| Error::Corrupt("record block is absent from directory".into()))?;
        Limits::check(
            "compressed block",
            block.compressed_size,
            self.limits.compressed,
        )?;
        Limits::check(
            "decompressed block",
            block.decompressed_size,
            self.limits.block,
        )?;
        let size = Self::address(block.decompressed_size)?;
        let start = self
            .offset
            .checked_add(block.start_pos)
            .ok_or_else(|| Error::Corrupt("record file position overflow".into()))?;
        let end = self
            .offset
            .checked_add(block.end_pos)
            .ok_or_else(|| Error::Corrupt("record file position overflow".into()))?;
        let compressed = self
            .data
            .as_ref()
            .get(Self::address(start)?..Self::address(end)?)
            .ok_or_else(|| Error::Corrupt("record block extends beyond file".into()))?;
        let cacheable = self.limits.cache != 0 && size != 0 && size <= self.limits.cache;
        if cacheable {
            cache.make_room(size, self.limits.cache);
        }
        let (decoded, _) = Block { size, limit: size }.decode(compressed)?;
        if decoded.len() != size {
            return Err(Error::Corrupt(
                "record block size disagrees with directory".into(),
            ));
        }
        let decoded: Arc<[u8]> = decoded.into();
        if cacheable {
            cache.resident += size;
            cache.blocks.put(id, Arc::clone(&decoded));
        }
        Ok(decoded)
    }
}

impl Reader<Mmap> {
    /// Open a memory-mapped dictionary without copying the source file.
    ///
    /// Uses default budgets and the file stem as a fallback title. This does not
    /// open a derived index cache or associated MDD volumes.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] on open or mapping failure, or any parsing
    /// error described by [`crate::Reader::parse`].
    ///
    /// # Safety
    /// The caller must ensure the file is not modified or truncated while this
    /// reader exists. Use [`crate::Reader::parse`] with owned bytes otherwise; advisory
    /// file locking alone does not establish this invariant.
    pub unsafe fn open(path: &Path) -> Result<Self> {
        // SAFETY: The caller guarantees that the backing file remains immutable.
        unsafe { Self::open_with(path, Limits::default()) }
    }

    /// Open an immutable memory-mapped dictionary with explicit budgets.
    ///
    /// Uses the file stem as a fallback title. No index sidecars are created.
    ///
    /// # Errors
    ///
    /// Returns I/O or parsing errors as described by [`crate::Reader::open`].
    ///
    /// # Safety
    /// The file must not be modified or truncated while this reader exists.
    pub unsafe fn open_with(path: &Path, limits: Limits) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("{}: {error}", path.display()),
            ))
        })?;
        // SAFETY: The caller guarantees the backing file remains immutable.
        let data = unsafe { Mmap::map(&file)? };
        let fallback = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("dictionary");
        Self::with_limits(data, fallback, limits)
    }
}

impl<B: AsRef<[u8]>> Read for Reader<B> {
    fn metadata(&self) -> &Metadata {
        &self.info
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn read(&self, id: usize) -> Result<Bytes> {
        let info = self
            .records
            .get(id)
            .ok_or_else(|| Error::Corrupt(format!("entry index {id} out of range")))?;
        Limits::check("record", info.record_size, self.limits.record)?;
        let length = Self::address(info.record_size)?;
        let skip = Self::address(info.record_offset)?;
        let relative = info
            .compressed_block_pos
            .checked_sub(self.offset)
            .ok_or_else(|| Error::Corrupt("record precedes record section".into()))?;
        let first = self
            .blocks
            .binary_search_by_key(&relative, |block| block.start_pos)
            .map_err(|_| Error::Corrupt("record block is absent from directory".into()))?;
        let decoded = self.block(first)?;
        let available = decoded
            .len()
            .checked_sub(skip)
            .ok_or_else(|| Error::Corrupt("record starts beyond decoded block".into()))?;
        if length <= available {
            return Bytes::view(decoded, skip..skip + length);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|error| Error::Corrupt(format!("record allocation: {error}")))?;
        output.extend_from_slice(&decoded[skip..]);
        drop(decoded);
        for id in first + 1..self.blocks.len() {
            let decoded = self.block(id)?;
            let take = decoded.len().min(length - output.len());
            output.extend_from_slice(&decoded[..take]);
            if output.len() == length {
                return Ok(output.into());
            }
        }
        Err(Error::Corrupt("record extends beyond record stream".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;
    use std::sync::Barrier;

    #[test]
    fn debug_keeps_large_sources_and_cached_records_bounded() {
        let payload = vec![b'x'; 1024 * 1024];
        let fixture = Fixture::new(&[payload.as_slice()], &[(0, "private-headword")]);
        let mut reader = Reader::parse(fixture.data, "fixture").unwrap();
        reader.info.title = "private-title".repeat(100_000);
        reader.info.description = "private-description".repeat(100_000);
        let bytes = reader.read(0).unwrap();
        assert_eq!(bytes.as_ref(), payload.as_slice());
        for rendered in [
            format!("{reader:?}"),
            format!("{reader:#?}"),
            format!("{:?}", reader.records()),
            format!("{:#?}", reader.records()),
        ] {
            assert!(rendered.len() < 512);
            assert!(!rendered.contains("private-headword"));
            assert!(!rendered.contains("private-title"));
            assert!(!rendered.contains("private-description"));
            assert!(!rendered.contains("120, 120"));
        }
    }

    struct Fixture {
        data: Vec<u8>,
        key_header: usize,
        record_header: usize,
        payload: usize,
    }

    impl Fixture {
        fn block(data: &[u8], compressed: bool) -> Vec<u8> {
            let mut bytes = Vec::new();
            let tag = if compressed { 0x0200_0000u32 } else { 0 };
            bytes.extend_from_slice(&tag.to_be_bytes());
            bytes.extend_from_slice(&adler::adler32_slice(data).to_be_bytes());
            if compressed {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(data).unwrap();
                bytes.extend_from_slice(&encoder.finish().unwrap());
            } else {
                bytes.extend_from_slice(data);
            }
            bytes
        }

        fn new(parts: &[&[u8]], entries: &[(u64, &str)]) -> Self {
            let xml = r#"<Dictionary GeneratedByEngineVersion="2.0" Encoding="UTF-8" Encrypted="0" Title="Fixture" />"#;
            let header: Vec<u8> = xml.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let mut data = Vec::new();
            data.extend_from_slice(&(header.len() as u32).to_be_bytes());
            data.extend_from_slice(&header);
            data.extend_from_slice(&adler::adler32_slice(&header).to_le_bytes());
            let key_header = data.len();
            let mut keys = Vec::new();
            for &(offset, word) in entries {
                keys.extend_from_slice(&offset.to_be_bytes());
                keys.extend_from_slice(word.as_bytes());
                keys.push(0);
            }
            let key_block = Self::block(&keys, true);
            let mut key_info = Vec::new();
            key_info.extend_from_slice(&(entries.len() as u64).to_be_bytes());
            for word in [entries.first().unwrap().1, entries.last().unwrap().1] {
                key_info.extend_from_slice(&(word.len() as u16).to_be_bytes());
                key_info.extend_from_slice(word.as_bytes());
                key_info.push(0);
            }
            key_info.extend_from_slice(&(key_block.len() as u64).to_be_bytes());
            key_info.extend_from_slice(&(keys.len() as u64).to_be_bytes());
            let encoded_info = Self::block(&key_info, true);
            for value in [
                1,
                entries.len() as u64,
                key_info.len() as u64,
                encoded_info.len() as u64,
                key_block.len() as u64,
            ] {
                data.extend_from_slice(&value.to_be_bytes());
            }
            data.extend_from_slice(&adler::adler32_slice(&data[key_header..]).to_be_bytes());
            data.extend_from_slice(&encoded_info);
            data.extend_from_slice(&key_block);
            let record_header = data.len();
            let mut directory = Vec::new();
            let mut records = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let block = Self::block(part, index % 2 == 0);
                directory.extend_from_slice(&(block.len() as u64).to_be_bytes());
                directory.extend_from_slice(&(part.len() as u64).to_be_bytes());
                records.extend_from_slice(&block);
            }
            for value in [
                parts.len() as u64,
                entries.len() as u64,
                directory.len() as u64,
                records.len() as u64,
            ] {
                data.extend_from_slice(&value.to_be_bytes());
            }
            data.extend_from_slice(&directory);
            let payload = data.len();
            data.extend_from_slice(&records);
            Self {
                data,
                key_header,
                record_header,
                payload,
            }
        }

        fn set_key_number(&mut self, field: usize, value: u64) {
            let start = self.key_header + field * 8;
            self.data[start..start + 8].copy_from_slice(&value.to_be_bytes());
            let end = self.key_header + 40;
            let checksum = adler::adler32_slice(&self.data[self.key_header..end]);
            self.data[end..end + 4].copy_from_slice(&checksum.to_be_bytes());
        }

        fn set_record_number(&mut self, field: usize, value: u64) {
            let start = self.record_header + field * 8;
            self.data[start..start + 8].copy_from_slice(&value.to_be_bytes());
        }

        fn reader(self, cache: usize) -> Reader<Vec<u8>> {
            Reader::with_limits(
                self.data,
                "fixture",
                Limits {
                    cache,
                    ..Limits::default()
                },
            )
            .unwrap()
        }
    }

    #[test]
    fn same_block_records_share_storage_and_decode_once() {
        let reader = Fixture::new(&[b"firstsecond"], &[(0, "first"), (5, "second")]).reader(11);
        let first = reader.read(0).unwrap();
        let second = reader.read(1).unwrap();
        let repeated = reader.read(0).unwrap();
        assert_eq!(first.as_ref(), b"first");
        assert_eq!(second.as_ref(), b"second");
        assert_eq!(
            first.as_ref().as_ptr().wrapping_add(5),
            second.as_ref().as_ptr()
        );
        assert_eq!(first.as_ref().as_ptr(), repeated.as_ref().as_ptr());
        let stats = reader.cache_stats();
        assert_eq!(
            (stats.misses, stats.hits, stats.resident, stats.blocks),
            (1, 2, 11, 1)
        );
    }

    #[test]
    fn records_cross_mixed_compression_blocks() {
        let reader = Fixture::new(
            &[b"xx<p>Hel", b"lo ", b"world</p>\0tail"],
            &[(2, "hello"), (21, "tail")],
        )
        .reader(64);
        assert_eq!(reader.read(0).unwrap().as_ref(), b"<p>Hello world</p>\0");
        assert_eq!(reader.text(0).unwrap(), "<p>Hello world</p>");
        assert_eq!(reader.text(1).unwrap(), "tail");
        assert_eq!(reader.cache_stats().misses, 3);
    }

    #[test]
    fn byte_budget_evicts_least_recent_without_invalidating_views() {
        let reader =
            Fixture::new(&[b"aaa", b"bbb", b"ccc"], &[(0, "a"), (3, "b"), (6, "c")]).reader(6);
        let retained = reader.read(0).unwrap();
        reader.read(1).unwrap();
        reader.read(0).unwrap();
        reader.read(2).unwrap();
        assert_eq!(reader.cache_stats().misses, 3);
        reader.read(1).unwrap();
        let stats = reader.cache_stats();
        assert_eq!(
            (stats.misses, stats.hits, stats.resident, stats.blocks),
            (4, 1, 6, 2)
        );
        assert_eq!(retained.as_ref(), b"aaa");
        let reloaded = reader.read(0).unwrap();
        assert_eq!(reloaded.as_ref(), retained.as_ref());
        assert_ne!(reloaded.as_ref().as_ptr(), retained.as_ref().as_ptr());
        assert!(reader.cache_stats().resident <= 6);
    }

    #[test]
    fn zero_and_undersized_caches_do_not_retain_blocks() {
        for budget in [0, 2] {
            let reader = Fixture::new(&[b"aaa"], &[(0, "a")]).reader(budget);
            assert_eq!(reader.read(0).unwrap().as_ref(), b"aaa");
            assert_eq!(reader.read(0).unwrap().as_ref(), b"aaa");
            let stats = reader.cache_stats();
            assert_eq!(
                (stats.misses, stats.hits, stats.resident, stats.blocks),
                (2, 0, 0, 0)
            );
        }
    }

    #[test]
    fn sequential_iteration_preserves_duplicates_aliases_and_order() {
        let reader = Fixture::new(
            &[b"firstsec", b"ond"],
            &[(0, "same"), (0, "alias"), (5, "same")],
        )
        .reader(11);
        let records: Vec<_> = reader
            .records()
            .map(|record| {
                let (entry, bytes) = record.unwrap();
                (entry.headword.clone(), bytes.as_ref().to_vec())
            })
            .collect();
        assert_eq!(
            records,
            [
                ("same".to_owned(), b"first".to_vec()),
                ("alias".to_owned(), b"first".to_vec()),
                ("same".to_owned(), b"second".to_vec()),
            ]
        );
        assert_eq!(reader.cache_stats().misses, 2);
    }

    #[test]
    fn iteration_advances_past_a_failed_record() {
        let mut fixture = Fixture::new(&[b"first", b"second"], &[(0, "first"), (5, "second")]);
        fixture.data[fixture.payload + 4] ^= 1;
        let reader = fixture.reader(64);
        let mut entries = reader.records();
        assert!(entries.next().unwrap().is_err());
        let (entry, bytes) = entries.next().unwrap().unwrap();
        assert_eq!(entry.headword, "second");
        assert_eq!(bytes.as_ref(), b"second");
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
    }

    #[test]
    fn concurrent_readers_decode_a_cacheable_block_once() {
        let reader = Fixture::new(&[b"firstsecond"], &[(0, "first"), (5, "second")]).reader(11);
        let barrier = Barrier::new(8);
        std::thread::scope(|scope| {
            for id in 0..8 {
                let reader = &reader;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let expected = if id % 2 == 0 {
                        &b"first"[..]
                    } else {
                        &b"second"[..]
                    };
                    assert_eq!(reader.read(id % 2).unwrap().as_ref(), expected);
                });
            }
        });
        let stats = reader.cache_stats();
        assert_eq!((stats.misses, stats.hits, stats.resident), (1, 7, 11));
    }

    #[test]
    fn strict_text_decoding_rejects_invalid_bytes_and_trims_terminators() {
        let reader = Fixture::new(&[b"good \n\0\xff"], &[(0, "good"), (7, "bad")]).reader(64);
        assert_eq!(reader.text(0).unwrap(), "good");
        assert!(matches!(reader.text(1), Err(Error::EncodingError(_))));
    }

    #[test]
    fn declared_counts_and_sizes_must_match_directories() {
        for field in [0, 1, 4] {
            let mut fixture = Fixture::new(&[b"record"], &[(0, "word")]);
            fixture.set_key_number(field, 2);
            assert!(Reader::parse(fixture.data, "fixture").is_err());
        }
        for field in 0..4 {
            let mut fixture = Fixture::new(&[b"record"], &[(0, "word")]);
            fixture.set_record_number(field, 2);
            assert!(matches!(
                Reader::parse(fixture.data, "fixture"),
                Err(Error::Corrupt(_))
            ));
        }
        let mut fixture = Fixture::new(&[b"record"], &[(0, "word")]);
        fixture.data.pop();
        assert!(matches!(
            Reader::parse(fixture.data, "fixture"),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn configurable_limits_reject_records_entries_blocks_and_metadata() {
        let parts = [&b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"[..]; 3];
        let fixture = Fixture::new(&parts, &[(0, "word")]);
        let bytes = fixture.data;
        for limits in [
            Limits {
                record: 95,
                ..Limits::default()
            },
            Limits {
                entries: 0,
                ..Limits::default()
            },
            Limits {
                blocks: 0,
                ..Limits::default()
            },
            Limits {
                block: 1,
                ..Limits::default()
            },
            Limits {
                compressed: 1,
                ..Limits::default()
            },
            Limits {
                metadata: 1,
                ..Limits::default()
            },
        ] {
            assert!(matches!(
                Reader::with_limits(&bytes, "fixture", limits),
                Err(Error::Limit { .. })
            ));
        }
        let reader = Reader::with_limits(
            bytes,
            "fixture",
            Limits {
                block: 64,
                record: 96,
                ..Limits::default()
            },
        )
        .unwrap();
        assert_eq!(reader.read(0).unwrap().as_ref(), &[b'a'; 96]);
    }
    #[test]
    fn cache_capacity_counts_bytes_not_block_count() {
        let reader =
            Fixture::new(&[b"aaa", b"bb", b"c"], &[(0, "a"), (3, "b"), (5, "c")]).reader(5);
        for id in 0..3 {
            reader.read(id).unwrap();
            assert!(reader.cache_stats().resident <= 5);
        }
        assert_eq!(reader.cache_stats().resident, 3);
        reader.read(1).unwrap();
        assert_eq!(reader.cache_stats().hits, 1);
        reader.read(0).unwrap();
        let stats = reader.cache_stats();
        assert_eq!((stats.misses, stats.resident, stats.blocks), (4, 5, 2));
    }

    #[test]
    fn entry_and_locator_allocations_share_the_metadata_budget() {
        let entries = vec![(0, "word"); 64];
        let fixture = Fixture::new(&[b"record"], &entries);
        let limits = Limits {
            metadata: 1024,
            ..Limits::default()
        };
        assert!(matches!(
            Reader::with_limits(fixture.data, "fixture", limits),
            Err(Error::Limit {
                kind: "metadata",
                ..
            })
        ));
    }
}
