//! Offline, original test data and a standalone MDX/MDD v2 writer.
//!
//! This module is shared by an integration test, an example and a benchmark;
//! each target intentionally uses a different subset of the corpus helpers.
#![allow(dead_code)]

use std::io::{self, Write};

use flate2::{Compression, write::ZlibEncoder};

#[derive(Clone, Copy)]
pub enum Kind {
    Articles,
    Resources,
}

impl Kind {
    fn append_key(self, output: &mut Vec<u8>, key: &str) {
        match self {
            Self::Articles => {
                output.extend_from_slice(key.as_bytes());
                output.push(0);
            }
            Self::Resources => {
                for unit in key.encode_utf16().chain(std::iter::once(0)) {
                    output.extend_from_slice(&unit.to_le_bytes());
                }
            }
        }
    }

    fn key_units(self, key: &str) -> usize {
        match self {
            Self::Articles => key.len(),
            Self::Resources => key.encode_utf16().count(),
        }
    }
}

/// Expected business data; article values do not include the wire terminator.
#[derive(Clone, Debug)]
pub struct Row {
    pub key: String,
    pub value: Vec<u8>,
}

impl Row {
    pub fn new(key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Data and block layout, independent of the binary serializer and the parser.
pub struct Corpus {
    pub rows: Vec<Row>,
    kind: Kind,
    title: &'static str,
    description: &'static str,
    key_records: usize,
    block_records: usize,
}

impl Corpus {
    pub const DEFAULT_ENTRIES: usize = 10_000;
    pub const MAX_ENTRIES: usize = 100_000;
    const MAX_PAYLOAD: usize = 128 * 1024 * 1024;

    pub fn articles(rows: Vec<Row>) -> Self {
        Self {
            rows,
            kind: Kind::Articles,
            title: "Offline fixture",
            description: "Original offline test data; no commercial dictionary content",
            key_records: 2,
            block_records: 2,
        }
    }

    pub fn resources(rows: Vec<Row>) -> Self {
        Self {
            kind: Kind::Resources,
            ..Self::articles(rows)
        }
    }

    pub fn basic_mdx() -> Self {
        Self::articles(vec![
            Row::new("apple", b"<p>A fruit.</p>"),
            Row::new("apples", b"@@@LINK=apple"),
            Row::new("apricot", b"<p>A stone fruit.</p>"),
            Row::new("banana", b"<p>A yellow fruit.</p>"),
            Row::new("café", b"<p>A cafe.</p>"),
            Row::new("猫", b"<p>A cat.</p>"),
        ])
    }

    pub fn basic_mdd() -> Self {
        Self::resources(vec![
            Row::new("\\hello.txt", b"hello resource"),
            Row::new("\\style.css", b"body { color: black; }"),
        ])
    }

    /// Fixed xorshift64 payloads, deliberately NOT the historical Python corpus.
    /// Every record is 511 ASCII bytes plus its MDX NUL, in blocks of 128 records.
    pub fn benchmark(count: usize) -> io::Result<Self> {
        Self::check_count(count)?;
        let mut state = 918_732u64;
        let mut rows = Vec::with_capacity(count);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for number in 0..count {
            let key = format!("word{number:08}");
            let mut value = Vec::with_capacity(511);
            value.extend_from_slice(key.as_bytes());
            value.push(b'|');
            while value.len() < 511 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                for byte in state.to_le_bytes() {
                    for nibble in [byte >> 4, byte & 15] {
                        if value.len() == 511 {
                            break;
                        }
                        value.push(HEX[nibble as usize]);
                    }
                }
            }
            rows.push(Row { key, value });
        }
        Ok(Self {
            rows,
            kind: Kind::Articles,
            title: "Rust offline benchmark",
            description: "xorshift64 seed 918732; 512-byte records; not the historical Python corpus",
            key_records: 512,
            block_records: 128,
        })
    }

    pub fn check_count(count: usize) -> io::Result<()> {
        if !(1..=Self::MAX_ENTRIES).contains(&count) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("entry count must be in 1..={}", Self::MAX_ENTRIES),
            ));
        }
        Ok(())
    }

    pub fn serialize(&self) -> io::Result<Vec<u8>> {
        Writer { corpus: self }.encode()
    }
}

struct Packed {
    bytes: Vec<u8>,
    raw_len: usize,
}

impl Packed {
    fn new(raw: &[u8]) -> io::Result<Self> {
        let mut bytes = Vec::new();
        // Compression marker is little-endian; the checksum is big-endian.
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&adler::adler32_slice(raw).to_be_bytes());
        let mut encoder = ZlibEncoder::new(bytes, Compression::new(6));
        encoder.write_all(raw)?;
        Ok(Self {
            bytes: encoder.finish()?,
            raw_len: raw.len(),
        })
    }
}

/// Serializes only the documented v2 layout; no wodix implementation imports.
struct Writer<'a> {
    corpus: &'a Corpus,
}

impl Writer<'_> {
    fn encode(&self) -> io::Result<Vec<u8>> {
        let corpus = self.corpus;
        Corpus::check_count(corpus.rows.len())?;
        let terminator = usize::from(matches!(corpus.kind, Kind::Articles));
        let mut offsets = Vec::with_capacity(corpus.rows.len());
        let mut record_bytes = 0usize;
        for row in &corpus.rows {
            if row.key.is_empty()
                || row.key.contains('\0')
                || corpus.kind.key_units(&row.key) > u16::MAX as usize
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "keys must be nonempty, NUL-free and fit the v2 u16 length field",
                ));
            }
            offsets.push(record_bytes as u64);
            record_bytes = record_bytes
                .checked_add(row.value.len())
                .and_then(|size| size.checked_add(terminator))
                .filter(|&size| size <= Corpus::MAX_PAYLOAD)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "corpus exceeds 128 MiB payload cap",
                    )
                })?;
        }

        let mut output = self.header();
        let mut key_blocks = Vec::new();
        let mut key_info = Vec::new();
        for (rows, offsets) in corpus
            .rows
            .chunks(corpus.key_records)
            .zip(offsets.chunks(corpus.key_records))
        {
            let mut raw = Vec::new();
            for (row, offset) in rows.iter().zip(offsets) {
                raw.extend_from_slice(&offset.to_be_bytes());
                corpus.kind.append_key(&mut raw, &row.key);
            }
            let block = Packed::new(&raw)?;
            key_info.extend_from_slice(&(rows.len() as u64).to_be_bytes());
            for row in [&rows[0], &rows[rows.len() - 1]] {
                let units = corpus.kind.key_units(&row.key) as u16;
                key_info.extend_from_slice(&units.to_be_bytes());
                corpus.kind.append_key(&mut key_info, &row.key);
            }
            key_info.extend_from_slice(&(block.bytes.len() as u64).to_be_bytes());
            key_info.extend_from_slice(&(block.raw_len as u64).to_be_bytes());
            key_blocks.push(block);
        }
        let packed_info = Packed::new(&key_info)?;
        let mut key_header = Vec::with_capacity(44);
        for number in [
            key_blocks.len() as u64,
            corpus.rows.len() as u64,
            key_info.len() as u64,
            packed_info.bytes.len() as u64,
            key_blocks
                .iter()
                .map(|block| block.bytes.len() as u64)
                .sum(),
        ] {
            key_header.extend_from_slice(&number.to_be_bytes());
        }
        let checksum = adler::adler32_slice(&key_header);
        key_header.extend_from_slice(&checksum.to_be_bytes());
        output.extend_from_slice(&key_header);
        output.extend_from_slice(&packed_info.bytes);
        for block in key_blocks {
            output.extend_from_slice(&block.bytes);
        }

        let mut record_blocks = Vec::new();
        for rows in corpus.rows.chunks(corpus.block_records) {
            let mut raw = Vec::new();
            for row in rows {
                raw.extend_from_slice(&row.value);
                if terminator != 0 {
                    raw.push(0);
                }
            }
            record_blocks.push(Packed::new(&raw)?);
        }
        for number in [
            record_blocks.len() as u64,
            corpus.rows.len() as u64,
            (record_blocks.len() * 16) as u64,
            record_blocks
                .iter()
                .map(|block| block.bytes.len() as u64)
                .sum(),
        ] {
            output.extend_from_slice(&number.to_be_bytes());
        }
        for block in &record_blocks {
            output.extend_from_slice(&(block.bytes.len() as u64).to_be_bytes());
            output.extend_from_slice(&(block.raw_len as u64).to_be_bytes());
        }
        for block in record_blocks {
            output.extend_from_slice(&block.bytes);
        }
        Ok(output)
    }

    fn header(&self) -> Vec<u8> {
        let (tag, encoding) = match self.corpus.kind {
            Kind::Articles => ("Dictionary", "UTF-8"),
            Kind::Resources => ("Library_Data", "UTF-16"),
        };
        let xml = format!(
            "<{tag} GeneratedByEngineVersion=\"2.0\" RequiredEngineVersion=\"2.0\" Encoding=\"{encoding}\" Encrypted=\"0\" Title=\"{}\" Description=\"{}\" Left2Right=\"Yes\" KeyCaseSensitive=\"Yes\" StripKey=\"No\" />",
            self.corpus.title, self.corpus.description,
        );
        let mut text = Vec::with_capacity((xml.len() + 1) * 2);
        for unit in xml.encode_utf16().chain(std::iter::once(0)) {
            text.extend_from_slice(&unit.to_le_bytes());
        }
        let mut header = Vec::with_capacity(text.len() + 8);
        header.extend_from_slice(&(text.len() as u32).to_be_bytes());
        header.extend_from_slice(&text);
        header.extend_from_slice(&adler::adler32_slice(&text).to_le_bytes());
        header
    }
}
