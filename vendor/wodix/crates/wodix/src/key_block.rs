//! Key Block Info and Key Block parsing.
//!
//! The key block section sits immediately after the header and contains:
//! 1. Key Block Header: counts and sizes
//! 2. Key Block Info: per-block (compressed, decompressed) size pairs
//! 3. Key Blocks: compressed blocks of (offset, headword) entries

use std::borrow::Cow;

use crate::Result;
use crate::checksum::{Adler, Verify};
use crate::decode::{Cursor, Decode};
use crate::decompress::Block;
use crate::decrypt::Cipher;
use crate::error::Error;
use crate::limits::Limits;
use crate::number::{Number, TextLen};
use crate::types::{Encoding, Encryption, Entry, Size, Version};

/// Parsed key block header metadata.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub num_blocks: u64,
    pub num_entries: u64,
    /// Only present in v2.0 — decompressed size of key block info.
    pub info_decompressed_size: Option<u64>,
    pub info_compressed_size: u64,
    pub blocks_total_size: u64,
}

pub struct KeyHeader {
    pub version: Version,
}

impl<'a> Decode<'a> for KeyHeader {
    type Output = KeyInfo;

    fn decode(&self, data: &'a [u8]) -> Result<(KeyInfo, usize)> {
        let mut cursor = Cursor { data, position: 0 };
        let number = Number {
            version: self.version,
        };
        let num_blocks = cursor.read(&number)?;
        let num_entries = cursor.read(&number)?;
        let info_decompressed_size = if self.version == Version::V2 {
            Some(cursor.read(&number)?)
        } else {
            None
        };
        let info_compressed_size = cursor.read(&number)?;
        let blocks_total_size = cursor.read(&number)?;

        if self.version == Version::V2 {
            let header_end = cursor.position;
            let checksum = cursor.take(4)?;
            Adler {
                expected: u32::from_be_bytes([checksum[0], checksum[1], checksum[2], checksum[3]]),
            }
            .verify(&data[..header_end])?;
        }

        Ok((
            KeyInfo {
                num_blocks,
                num_entries,
                info_decompressed_size,
                info_compressed_size,
                blocks_total_size,
            },
            cursor.position,
        ))
    }
}

pub struct Keys<'e> {
    pub version: Version,
    pub encoding: &'e Encoding,
    pub encrypted: Encryption,
    pub limits: &'e Limits,
}

impl Keys<'_> {
    /// Decode the possibly encrypted and compressed key block metadata.
    pub fn info(
        &self,
        raw: &[u8],
        decompressed_size: Option<u64>,
        expected_entries: u64,
    ) -> Result<Vec<Size>> {
        Limits::check("entries", expected_entries, self.limits.entries)?;
        if self.encrypted.0 & !2 != 0 || (self.version == Version::V1 && self.encrypted.0 != 0) {
            return Err(Error::UnsupportedEncryption(self.encrypted.0));
        }
        Limits::check("key index", raw.len() as u64, self.limits.key_index)?;
        Limits::check("compressed", raw.len() as u64, self.limits.compressed)?;
        Limits::check(
            "key index",
            decompressed_size.unwrap_or(0),
            self.limits.key_index,
        )?;
        let data = if self.version == Version::V2 {
            let compressed = if self.encrypted.0 & 2 != 0 {
                let mut bytes = raw.to_vec();
                Cipher::apply(&mut bytes)?;
                Cow::Owned(bytes)
            } else {
                Cow::Borrowed(raw)
            };
            let size = usize::try_from(decompressed_size.unwrap_or(0))
                .map_err(|_| Error::Corrupt("key info size exceeds address space".into()))?;
            Cow::Owned(
                Block {
                    size,
                    limit: decompressed_size.map_or(self.limits.key_index, |_| size),
                }
                .decode(&compressed)?
                .0,
            )
        } else {
            Cow::Borrowed(raw)
        };
        self.sizes(&data, expected_entries)
    }

    fn sizes(&self, data: &[u8], expected_entries: u64) -> Result<Vec<Size>> {
        Limits::check("key index", data.len() as u64, self.limits.key_index)?;
        let mut cursor = Cursor { data, position: 0 };
        let number = Number {
            version: self.version,
        };
        let text_len = TextLen {
            version: self.version,
        };
        let terminator = usize::from(self.version == Version::V2);
        let width = if *self.encoding == Encoding::Utf16Le {
            2
        } else {
            1
        };
        let mut sizes = Vec::new();
        let mut entries = 0u64;

        while cursor.position < data.len() {
            self.reserve(&mut sizes, self.limits.blocks, "blocks", 0)?;
            entries = entries
                .checked_add(cursor.read(&number)?)
                .ok_or_else(|| Error::Corrupt("key entry count overflow".into()))?;
            Limits::check("entries", entries, self.limits.entries)?;
            for _ in 0..2 {
                let len = usize::try_from(cursor.read(&text_len)?)
                    .ok()
                    .and_then(|len| len.checked_add(terminator))
                    .and_then(|len| len.checked_mul(width))
                    .ok_or_else(|| Error::Corrupt("key info text size overflow".into()))?;
                let text = cursor.take(len)?;
                let text_end = len - terminator * width;
                if text[text_end..].iter().any(|byte| *byte != 0) {
                    return Err(Error::Corrupt("invalid key info text terminator".into()));
                }
                self.text(
                    &text[..text_end],
                    sizes.capacity() * std::mem::size_of::<Size>(),
                )?;
            }
            let compressed = cursor.read(&number)?;
            let decompressed = cursor.read(&number)?;
            Limits::check("compressed", compressed, self.limits.compressed)?;
            Limits::check("block", decompressed, self.limits.block)?;
            sizes.push(Size {
                compressed,
                decompressed,
            });
        }
        if entries != expected_entries {
            return Err(Error::Corrupt("key info entry count mismatch".into()));
        }
        Ok(sizes)
    }

    /// Split headwords, advancing by source bytes rather than decoded string length.
    pub fn entries(&self, block: &[u8]) -> Result<Vec<Entry>> {
        Limits::check("block", block.len() as u64, self.limits.block)?;
        let mut cursor = Cursor {
            data: block,
            position: 0,
        };
        let number = Number {
            version: self.version,
        };
        let mut entries = Vec::new();
        let mut text_bytes = 0usize;
        while cursor.position < block.len() {
            self.reserve(&mut entries, self.limits.entries, "entries", text_bytes)?;
            let record_offset = cursor.read(&number)?;
            let metadata = text_bytes
                .checked_add(entries.capacity() * std::mem::size_of::<Entry>())
                .ok_or_else(|| Error::Corrupt("key metadata size overflow".into()))?;
            let (headword, consumed) = self.headword(&block[cursor.position..], metadata)?;
            text_bytes = text_bytes
                .checked_add(headword.capacity())
                .ok_or_else(|| Error::Corrupt("key text size overflow".into()))?;
            cursor.take(consumed)?;
            entries.push(Entry {
                record_offset,
                headword,
            });
        }
        Ok(entries)
    }

    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        limit: usize,
        kind: &'static str,
        text_bytes: usize,
    ) -> Result<()> {
        let count = values
            .len()
            .checked_add(1)
            .ok_or_else(|| Error::Corrupt("key count overflow".into()))?;
        Limits::check(kind, count as u64, limit)?;
        let width = std::mem::size_of::<T>();
        let needed = count
            .checked_mul(width)
            .and_then(|bytes| bytes.checked_add(text_bytes))
            .ok_or_else(|| Error::Corrupt("key metadata size overflow".into()))?;
        Limits::check("metadata", needed as u64, self.limits.metadata)?;
        if values.len() == values.capacity() {
            let capacity = values
                .capacity()
                .saturating_mul(2)
                .max(4)
                .min(limit)
                .min((self.limits.metadata - text_bytes) / width);
            values
                .try_reserve_exact(capacity - values.len())
                .map_err(|error| Error::Corrupt(format!("key directory allocation: {error}")))?;
        }
        Ok(())
    }

    fn text<'a>(&self, bytes: &'a [u8], metadata: usize) -> Result<Cow<'a, str>> {
        let expansion = if *self.encoding == Encoding::Utf8 {
            1
        } else {
            3
        };
        let maximum = bytes
            .len()
            .checked_mul(expansion)
            .and_then(|size| size.checked_add(metadata))
            .ok_or_else(|| Error::Corrupt("key text size overflow".into()))?;
        Limits::check("metadata", maximum as u64, self.limits.metadata)?;
        let text = self.encoding.decode(bytes)?;
        let capacity = match &text {
            Cow::Borrowed(text) => text.len(),
            Cow::Owned(text) => text.capacity(),
        };
        let actual = metadata
            .checked_add(capacity)
            .ok_or_else(|| Error::Corrupt("key text size overflow".into()))?;
        Limits::check("metadata", actual as u64, self.limits.metadata)?;
        Ok(text)
    }

    fn headword(&self, data: &[u8], metadata: usize) -> Result<(String, usize)> {
        let width = if *self.encoding == Encoding::Utf16Le {
            2
        } else {
            1
        };
        let end = data
            .chunks_exact(width)
            .position(|unit| unit.iter().all(|byte| *byte == 0))
            .ok_or_else(|| Error::Corrupt("unterminated headword".into()))?
            * width;
        Ok((self.text(&data[..end], metadata)?.into_owned(), end + width))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adler::adler32_slice;

    struct Fixture;

    impl Fixture {
        fn block(data: &[u8]) -> Vec<u8> {
            let mut block = Vec::with_capacity(data.len() + 8);
            block.extend_from_slice(&0u32.to_be_bytes());
            block.extend_from_slice(&adler32_slice(data).to_be_bytes());
            block.extend_from_slice(data);
            block
        }

        fn info() -> Vec<u8> {
            let mut data = 1u64.to_be_bytes().to_vec();
            for word in [b"a", b"z"] {
                data.extend_from_slice(&1u16.to_be_bytes());
                data.extend_from_slice(word);
                data.push(0);
            }
            data.extend_from_slice(&10u64.to_be_bytes());
            data.extend_from_slice(&20u64.to_be_bytes());
            data
        }

        fn encrypted(mut block: Vec<u8>) -> Vec<u8> {
            use ripemd::{Digest, Ripemd128};
            let mut hasher = Ripemd128::new();
            hasher.update(&block[4..8]);
            hasher.update([0x95, 0x36, 0, 0]);
            let key: [u8; 16] = hasher.finalize().into();
            let mut previous = 0x36;
            for (index, byte) in block[8..].iter_mut().enumerate() {
                *byte = (*byte ^ previous ^ index as u8 ^ key[index % 16]).rotate_left(4);
                previous = *byte;
            }
            block
        }
    }

    #[test]
    fn utf8_multibyte_preserves_next_entry_boundary() {
        let mut block = 0u64.to_be_bytes().to_vec();
        block.extend_from_slice("你好\0".as_bytes());
        block.extend_from_slice(&42u64.to_be_bytes());
        block.extend_from_slice(b"next\0");
        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries[0].headword, "你好");
        assert_eq!(entries[1].headword, "next");
        assert_eq!(entries[1].record_offset, 42);
    }

    #[test]
    fn gb18030_headwords_use_source_byte_boundaries() {
        let mut block = 0u64.to_be_bytes().to_vec();
        block.extend_from_slice(&[0xc4, 0xe3, 0xba, 0xc3, 0]);
        block.extend_from_slice(&42u64.to_be_bytes());
        block.extend_from_slice(b"next\0");
        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Gb18030,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries[0].headword, "你好");
        assert_eq!(entries[1].headword, "next");
        assert_eq!(entries[1].record_offset, 42);
    }

    #[test]
    fn rejects_unterminated_headwords() {
        for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Gb18030] {
            let mut block = 0u64.to_be_bytes().to_vec();
            block.extend_from_slice(b"ab");
            assert!(matches!(
                Keys {
                    version: Version::V2,
                    encoding: &encoding,
                    encrypted: Encryption::NONE,
                    limits: &Limits::default(),
                }
                .entries(&block),
                Err(Error::Corrupt(_))
            ));
        }
    }

    #[test]
    fn rejects_truncated_metadata_text() {
        let mut data = 1u32.to_be_bytes().to_vec();
        data.push(255);
        data.extend_from_slice(b"short");
        assert!(matches!(
            Keys {
                version: Version::V1,
                encoding: &Encoding::Utf8,
                encrypted: Encryption::NONE,
                limits: &Limits::default(),
            }
            .info(&data, None, 1),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn parse_key_block_header_v2() {
        // 5 × i64 + u32 checksum
        let mut data = Vec::new();
        data.extend_from_slice(&10u64.to_be_bytes()); // num_blocks
        data.extend_from_slice(&1000u64.to_be_bytes()); // num_entries
        data.extend_from_slice(&256u64.to_be_bytes()); // info_decompressed_size
        data.extend_from_slice(&128u64.to_be_bytes()); // info_compressed_size
        data.extend_from_slice(&4096u64.to_be_bytes()); // blocks_total_size

        let checksum = adler32_slice(&data);
        data.extend_from_slice(&checksum.to_be_bytes());

        let (header, consumed) = KeyHeader {
            version: Version::V2,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(header.num_blocks, 10);
        assert_eq!(header.num_entries, 1000);
        assert_eq!(header.info_decompressed_size, Some(256));
        assert_eq!(header.info_compressed_size, 128);
        assert_eq!(header.blocks_total_size, 4096);
        assert_eq!(consumed, 44); // 5*8 + 4
    }

    #[test]
    fn parse_key_block_header_v1() {
        // 4 × u32, no checksum
        let mut data = Vec::new();
        data.extend_from_slice(&5u32.to_be_bytes()); // num_blocks
        data.extend_from_slice(&500u32.to_be_bytes()); // num_entries
        data.extend_from_slice(&64u32.to_be_bytes()); // info_compressed_size
        data.extend_from_slice(&2048u32.to_be_bytes()); // blocks_total_size

        let (header, consumed) = KeyHeader {
            version: Version::V1,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(header.num_blocks, 5);
        assert_eq!(header.num_entries, 500);
        assert_eq!(header.info_decompressed_size, None);
        assert_eq!(header.info_compressed_size, 64);
        assert_eq!(header.blocks_total_size, 2048);
        assert_eq!(consumed, 16);
    }

    #[test]
    fn parse_key_block_header_bad_checksum() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(&0xDEADBEEFu32.to_be_bytes()); // bad checksum

        let result = KeyHeader {
            version: Version::V2,
        }
        .decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn split_headwords_utf8_v2() {
        let mut block = Vec::new();
        // Entry 1: offset=0, headword="hello"
        block.extend_from_slice(&0u64.to_be_bytes());
        block.extend_from_slice(b"hello\0");
        // Entry 2: offset=100, headword="world"
        block.extend_from_slice(&100u64.to_be_bytes());
        block.extend_from_slice(b"world\0");

        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].record_offset, 0);
        assert_eq!(entries[0].headword, "hello");
        assert_eq!(entries[1].record_offset, 100);
        assert_eq!(entries[1].headword, "world");
    }

    #[test]
    fn split_headwords_utf16le_v2() {
        let mut block = Vec::new();
        // Entry: offset=42, headword="hi" in UTF-16LE
        block.extend_from_slice(&42u64.to_be_bytes());
        // 'h' = 0x0068, 'i' = 0x0069, null = 0x0000
        block.extend_from_slice(&[0x68, 0x00, 0x69, 0x00, 0x00, 0x00]);

        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf16Le,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].record_offset, 42);
        assert_eq!(entries[0].headword, "hi");
    }

    #[test]
    fn split_headwords_utf8_v1() {
        let mut block = Vec::new();
        // Entry: offset=10, headword="test"
        block.extend_from_slice(&10u32.to_be_bytes());
        block.extend_from_slice(b"test\0");

        let entries = Keys {
            version: Version::V1,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].record_offset, 10);
        assert_eq!(entries[0].headword, "test");
    }

    #[test]
    fn decode_key_block_info_v1() {
        // v1: each entry is: num_keywords(u32) + first_head_size(u8) + text + last_head_size(u8) + text + comp(u32) + decomp(u32)
        let mut data = Vec::new();
        data.extend_from_slice(&50u32.to_be_bytes()); // num_keywords
        data.push(3); // first_head_size = 3
        data.extend_from_slice(b"abc"); // 3 bytes
        data.push(5); // last_head_size = 5
        data.extend_from_slice(b"zebra"); // 5 bytes
        data.extend_from_slice(&100u32.to_be_bytes()); // compressed
        data.extend_from_slice(&200u32.to_be_bytes()); // decompressed

        let entries = Keys {
            version: Version::V1,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .info(&data, None, 50)
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].compressed, 100);
        assert_eq!(entries[0].decompressed, 200);
    }

    #[test]
    fn decode_key_block_info_v2_utf8() {
        // v2: num(i64) + head_size(u16) + text(size+1 bytes) + ... + comp(i64) + decomp(i64)
        let mut data = Vec::new();
        data.extend_from_slice(&100u64.to_be_bytes()); // num_keywords
        data.extend_from_slice(&3u16.to_be_bytes()); // first_head_size = 3
        data.extend_from_slice(b"abc\0"); // 3+1 = 4 bytes (textTermSize=1)
        data.extend_from_slice(&4u16.to_be_bytes()); // last_head_size = 4
        data.extend_from_slice(b"wxyz\0"); // 4+1 = 5 bytes
        data.extend_from_slice(&512u64.to_be_bytes()); // compressed
        data.extend_from_slice(&1024u64.to_be_bytes()); // decompressed

        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .info(&Fixture::block(&data), Some(data.len() as u64), 100)
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].compressed, 512);
        assert_eq!(entries[0].decompressed, 1024);
    }

    #[test]
    fn decode_key_block_info_v2_utf16le() {
        // v2 + UTF-16LE: text bytes = (size+1)*2
        let mut data = Vec::new();
        data.extend_from_slice(&10u64.to_be_bytes()); // num_keywords
        data.extend_from_slice(&2u16.to_be_bytes()); // first_head_size = 2 chars
        // 2+1 = 3 chars × 2 = 6 bytes
        data.extend_from_slice(&[0x41, 0x00, 0x42, 0x00, 0x00, 0x00]); // "AB\0"
        data.extend_from_slice(&1u16.to_be_bytes()); // last_head_size = 1 char
        // 1+1 = 2 chars × 2 = 4 bytes
        data.extend_from_slice(&[0x5A, 0x00, 0x00, 0x00]); // "Z\0"
        data.extend_from_slice(&256u64.to_be_bytes()); // compressed
        data.extend_from_slice(&768u64.to_be_bytes()); // decompressed

        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf16Le,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .info(&Fixture::block(&data), Some(data.len() as u64), 10)
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].compressed, 256);
        assert_eq!(entries[0].decompressed, 768);
    }

    #[test]
    fn invalid_headword_text_is_not_replaced() {
        for (encoding, bytes) in [
            (Encoding::Utf8, &[0xff, 0][..]),
            (Encoding::Utf16Le, &[0, 0xd8, 0, 0][..]),
            (Encoding::Gb18030, &[0x81, 0][..]),
            (Encoding::Big5, &[0xa4, 0][..]),
        ] {
            let mut block = 0u64.to_be_bytes().to_vec();
            block.extend_from_slice(bytes);
            let limits = Limits::default();
            let keys = Keys {
                version: Version::V2,
                encoding: &encoding,
                encrypted: Encryption::NONE,
                limits: &limits,
            };
            assert!(matches!(keys.entries(&block), Err(Error::EncodingError(_))));
        }
    }

    #[test]
    fn big5_headwords_preserve_next_offset() {
        let mut block = 0u64.to_be_bytes().to_vec();
        block.extend_from_slice(&[0xa4, 0xa4, 0xa4, 0xe5, 0]);
        block.extend_from_slice(&42u64.to_be_bytes());
        block.extend_from_slice(b"next\0");
        let entries = Keys {
            version: Version::V2,
            encoding: &Encoding::Big5,
            encrypted: Encryption::NONE,
            limits: &Limits::default(),
        }
        .entries(&block)
        .unwrap();
        assert_eq!(entries[0].headword, "中文");
        assert_eq!(entries[1].headword, "next");
        assert_eq!(entries[1].record_offset, 42);
    }

    #[test]
    fn encrypted_key_info_retains_metadata() {
        let data = Fixture::info();
        let encrypted = Fixture::encrypted(Fixture::block(&data));
        let sizes = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf8,
            encrypted: Encryption(2),
            limits: &Limits::default(),
        }
        .info(&encrypted, Some(data.len() as u64), 1)
        .unwrap();
        assert_eq!(
            sizes,
            [Size {
                compressed: 10,
                decompressed: 20
            }]
        );
    }

    #[test]
    fn key_metadata_text_and_terminators_are_strict() {
        for invalid in [0xff, 0x81] {
            let mut data = Fixture::info();
            data[10] = invalid;
            assert!(matches!(
                Keys {
                    version: Version::V2,
                    encoding: &Encoding::Utf8,
                    encrypted: Encryption::NONE,
                    limits: &Limits::default()
                }
                .info(&Fixture::block(&data), Some(data.len() as u64), 1),
                Err(Error::EncodingError(_))
            ));
        }
        let mut data = Fixture::info();
        data[11] = b'x';
        assert!(matches!(
            Keys {
                version: Version::V2,
                encoding: &Encoding::Utf8,
                encrypted: Encryption::NONE,
                limits: &Limits::default()
            }
            .info(&Fixture::block(&data), Some(data.len() as u64), 1),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn key_limits_precede_allocations() {
        let data = Fixture::info();
        for limits in [
            Limits {
                blocks: 0,
                ..Limits::default()
            },
            Limits {
                entries: 0,
                ..Limits::default()
            },
            Limits {
                key_index: 1,
                ..Limits::default()
            },
            Limits {
                metadata: 1,
                ..Limits::default()
            },
            Limits {
                compressed: 9,
                ..Limits::default()
            },
            Limits {
                block: 19,
                ..Limits::default()
            },
        ] {
            assert!(matches!(
                Keys {
                    version: Version::V2,
                    encoding: &Encoding::Utf8,
                    encrypted: Encryption::NONE,
                    limits: &limits
                }
                .info(&Fixture::block(&data), Some(data.len() as u64), 1),
                Err(Error::Limit { .. })
            ));
        }
        let mut block = 0u64.to_be_bytes().to_vec();
        block.extend_from_slice(b"word\0");
        for limits in [
            Limits {
                entries: 0,
                ..Limits::default()
            },
            Limits {
                metadata: std::mem::size_of::<Entry>() + 3,
                ..Limits::default()
            },
        ] {
            assert!(matches!(
                Keys {
                    version: Version::V2,
                    encoding: &Encoding::Utf8,
                    encrypted: Encryption::NONE,
                    limits: &limits
                }
                .entries(&block),
                Err(Error::Limit { .. })
            ));
        }
    }

    #[test]
    fn key_info_counts_must_match_the_section_header() {
        let data = Fixture::info();
        for expected in [0, 2] {
            assert!(matches!(
                Keys {
                    version: Version::V2,
                    encoding: &Encoding::Utf8,
                    encrypted: Encryption::NONE,
                    limits: &Limits::default()
                }
                .info(&Fixture::block(&data), Some(data.len() as u64), expected),
                Err(Error::Corrupt(_))
            ));
        }
    }

    #[test]
    fn declared_zero_key_info_is_not_an_unknown_size() {
        let data = Fixture::info();
        let limits = Limits::default();
        let keys = Keys {
            version: Version::V2,
            encoding: &Encoding::Utf8,
            encrypted: Encryption::NONE,
            limits: &limits,
        };
        assert!(matches!(
            keys.info(&Fixture::block(&data), Some(0), 1),
            Err(Error::Limit { limit: 0, .. })
        ));
    }
}
