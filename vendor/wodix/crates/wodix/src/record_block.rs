//! Record block directories and shared, immutable record bytes.

use std::ops::Range;
use std::sync::Arc;

use crate::decode::{Cursor, Decode};
use crate::number::Number;
use crate::types::{Entry, Locator, Span, Version};
use crate::{Error, Limits, Result};

/// Raw, immutable decompressed record bytes with cheap shared ownership.
///
/// A record contained in one block shares that block; a cross-block record is
/// assembled into owned storage. Access the selected bytes with [`AsRef::as_ref`].
/// No text decoding, NUL trimming, redirect resolution or style expansion is
/// performed. See [`crate::Read::text`] and [`crate::Mdx::lookup`] for those layers.
///
/// Live views remain valid after cache eviction. A small view can retain an
/// entire decoded block; caller-retained memory is outside the reader's cache
/// budget. `Debug` reports lengths only and never includes payload bytes.
#[derive(Clone)]
pub struct Bytes {
    data: Arc<[u8]>,
    range: Range<usize>,
}

impl std::fmt::Debug for Bytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Bytes")
            .field("len", &self.range.len())
            .field("backing_len", &self.data.len())
            .finish_non_exhaustive()
    }
}

impl Bytes {
    pub(crate) fn view(data: Arc<[u8]>, range: Range<usize>) -> Result<Self> {
        if data.get(range.clone()).is_none() {
            return Err(Error::Corrupt("record slice exceeds decoded block".into()));
        }
        Ok(Self { data, range })
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(data: Vec<u8>) -> Self {
        let range = 0..data.len();
        Self {
            data: data.into(),
            range,
        }
    }
}

/// Parsed record block section header.
#[derive(Debug, Clone)]
pub struct RecordInfo {
    pub num_record_blocks: u64,
    pub num_entries: u64,
    pub record_info_size: u64,
    pub total_records_size: u64,
}

pub struct RecordHeader {
    pub version: Version,
}

impl<'a> Decode<'a> for RecordHeader {
    type Output = RecordInfo;

    fn decode(&self, data: &'a [u8]) -> Result<(RecordInfo, usize)> {
        let mut cursor = Cursor { data, position: 0 };
        let number = Number {
            version: self.version,
        };
        let info = RecordInfo {
            num_record_blocks: cursor.read(&number)?,
            num_entries: cursor.read(&number)?,
            record_info_size: cursor.read(&number)?,
            total_records_size: cursor.read(&number)?,
        };
        Ok((info, cursor.position))
    }
}

pub struct Records<'b> {
    pub blocks: &'b [Span],
    pub offset: u64,
}

impl Records<'_> {
    /// Build physical and logical ranges without touching record payloads.
    pub fn index(
        data: &[u8],
        num_blocks: u64,
        version: Version,
        limits: &Limits,
    ) -> Result<Vec<Span>> {
        Limits::check("record blocks", num_blocks, limits.blocks)?;
        let count = usize::try_from(num_blocks)
            .map_err(|_| Error::Corrupt("record block count exceeds address space".into()))?;
        let table_size = count
            .checked_mul(version.number_size() * 2)
            .ok_or_else(|| Error::Corrupt("record directory size overflow".into()))?;
        if table_size != data.len() {
            return Err(Error::Corrupt(
                "record directory size disagrees with block count".into(),
            ));
        }
        let allocation = count
            .checked_mul(std::mem::size_of::<Span>())
            .ok_or_else(|| Error::Corrupt("record directory allocation overflow".into()))?;
        Limits::check("metadata", allocation as u64, limits.metadata)?;
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(count)
            .map_err(|error| Error::Corrupt(format!("record directory allocation: {error}")))?;
        let mut cursor = Cursor { data, position: 0 };
        let number = Number { version };
        let mut start_pos = 0u64;
        let mut shadow_start = 0u64;
        for _ in 0..count {
            let compressed_size = cursor.read(&number)?;
            let decompressed_size = cursor.read(&number)?;
            Limits::check("compressed block", compressed_size, limits.compressed)?;
            Limits::check("decompressed block", decompressed_size, limits.block)?;
            if compressed_size < 8 {
                return Err(Error::Corrupt(
                    "record block lacks compression header".into(),
                ));
            }
            let end_pos = start_pos
                .checked_add(compressed_size)
                .ok_or_else(|| Error::Corrupt("compressed record positions overflow".into()))?;
            let shadow_end = shadow_start
                .checked_add(decompressed_size)
                .ok_or_else(|| Error::Corrupt("decompressed record positions overflow".into()))?;
            blocks.push(Span {
                start_pos,
                end_pos,
                shadow_start,
                shadow_end,
                compressed_size,
                decompressed_size,
            });
            start_pos = end_pos;
            shadow_start = shadow_end;
        }
        Ok(blocks)
    }

    /// Equal offsets name the same record; its boundary is the next distinct offset.
    pub fn locate(&self, entries: &[Entry], limits: &Limits) -> Result<Vec<Locator>> {
        Limits::check("entries", entries.len() as u64, limits.entries)?;
        let allocation = entries
            .len()
            .checked_mul(std::mem::size_of::<Locator>())
            .ok_or_else(|| Error::Corrupt("record locator allocation overflow".into()))?;
        Limits::check("metadata", allocation as u64, limits.metadata)?;
        let mut infos = Vec::new();
        infos
            .try_reserve_exact(entries.len())
            .map_err(|error| Error::Corrupt(format!("record locator allocation: {error}")))?;
        let stream_end = self.blocks.last().map_or(0, |block| block.shadow_end);
        let mut first = 0;
        while first < entries.len() {
            let start = entries[first].record_offset;
            let mut next = first + 1;
            while next < entries.len() && entries[next].record_offset == start {
                next += 1;
            }
            let block_idx = Span::find(self.blocks, start)
                .ok_or_else(|| Error::Corrupt(format!("record offset {start} not in any block")))?;
            let block = &self.blocks[block_idx];
            let end = entries
                .get(next)
                .map_or(stream_end, |entry| entry.record_offset);
            let record_size = end
                .checked_sub(start)
                .filter(|_| end <= stream_end)
                .ok_or_else(|| Error::Corrupt("invalid record offset ordering or extent".into()))?;
            Limits::check("record", record_size, limits.record)?;
            let compressed_block_pos = self
                .offset
                .checked_add(block.start_pos)
                .ok_or_else(|| Error::Corrupt("record file position overflow".into()))?;
            let record_offset = start
                .checked_sub(block.shadow_start)
                .ok_or_else(|| Error::Corrupt("record precedes its block".into()))?;
            infos.resize(
                next,
                Locator {
                    compressed_block_pos,
                    record_offset,
                    record_size,
                },
            );
            first = next;
        }
        Ok(infos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_does_not_dump_shared_payload() {
        let payload: Arc<[u8]> = vec![b'x'; 1024 * 1024].into();
        let bytes = Bytes::view(payload, 100..101).unwrap();
        for rendered in [format!("{bytes:?}"), format!("{bytes:#?}")] {
            assert!(rendered.len() < 256);
            assert!(!rendered.contains("xxxxxxxx"));
            assert!(!rendered.contains("120, 120"));
        }
    }

    struct Fixture;

    impl Fixture {
        fn directory(sizes: &[(u64, u64)]) -> Vec<u8> {
            let mut bytes = Vec::new();
            for &(compressed, decompressed) in sizes {
                bytes.extend_from_slice(&compressed.to_be_bytes());
                bytes.extend_from_slice(&decompressed.to_be_bytes());
            }
            bytes
        }

        fn entries(offsets: &[u64]) -> Vec<Entry> {
            offsets
                .iter()
                .map(|&record_offset| Entry {
                    record_offset,
                    headword: record_offset.to_string(),
                })
                .collect()
        }
    }

    #[test]
    fn aliases_share_extent_across_blocks() {
        let limits = Limits::default();
        let blocks = Records::index(
            &Fixture::directory(&[(20, 8), (30, 4)]),
            2,
            Version::V2,
            &limits,
        )
        .unwrap();
        let entries = Fixture::entries(&[0, 0, 5, 5]);
        let locators = Records {
            blocks: &blocks,
            offset: 100,
        }
        .locate(&entries, &limits)
        .unwrap();
        assert_eq!(
            locators
                .iter()
                .map(|item| item.record_size)
                .collect::<Vec<_>>(),
            [5, 5, 7, 7]
        );
        assert_eq!(locators[2].record_offset, 5);
        assert_eq!(locators[2].compressed_block_pos, 100);
    }

    #[test]
    fn rejects_invalid_offsets_and_directory_extents() {
        let limits = Limits::default();
        let directory = Fixture::directory(&[(20, 8)]);
        let blocks = Records::index(&directory, 1, Version::V2, &limits).unwrap();
        let records = Records {
            blocks: &blocks,
            offset: 0,
        };
        for offsets in [&[4, 1][..], &[8][..], &[9][..]] {
            assert!(matches!(
                records.locate(&Fixture::entries(offsets), &limits),
                Err(Error::Corrupt(_))
            ));
        }
        assert!(matches!(
            Records {
                blocks: &[],
                offset: 0
            }
            .locate(&Fixture::entries(&[0]), &limits),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            Records::index(&directory, 0, Version::V2, &limits),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            Records::index(&directory[..15], 1, Version::V2, &limits),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn rejects_budget_overruns_before_allocating_directories() {
        let limits = Limits {
            blocks: 1,
            record: 7,
            ..Limits::default()
        };
        assert!(matches!(
            Records::index(&[], 2, Version::V2, &limits),
            Err(Error::Limit { .. })
        ));
        let directory = Fixture::directory(&[(20, 8)]);
        let blocks = Records::index(&directory, 1, Version::V2, &limits).unwrap();
        assert!(matches!(
            Records {
                blocks: &blocks,
                offset: 0
            }
            .locate(&Fixture::entries(&[0]), &limits),
            Err(Error::Limit { .. })
        ));
        let small = Limits {
            metadata: 1,
            ..limits
        };
        assert!(matches!(
            Records::index(&directory, 1, Version::V2, &small),
            Err(Error::Limit { .. })
        ));
    }

    #[test]
    fn retains_u64_physical_and_logical_offsets() {
        let limits = Limits {
            block: usize::MAX,
            compressed: usize::MAX,
            record: usize::MAX,
            ..Limits::default()
        };
        let large = u64::from(u32::MAX) + 100;
        let blocks = Records::index(
            &Fixture::directory(&[(large, large), (20, 3)]),
            2,
            Version::V2,
            &limits,
        )
        .unwrap();
        let infos = Records {
            blocks: &blocks,
            offset: 1000,
        }
        .locate(&Fixture::entries(&[large]), &limits)
        .unwrap();
        assert_eq!(infos[0].compressed_block_pos, large + 1000);
        assert_eq!(infos[0].record_size, 3);
    }

    #[test]
    fn rejects_accumulation_overflow() {
        let limits = Limits {
            block: usize::MAX,
            compressed: usize::MAX,
            ..Limits::default()
        };
        for sizes in [[(u64::MAX, 1), (8, 1)], [(8, u64::MAX), (8, 1)]] {
            assert!(Records::index(&Fixture::directory(&sizes), 2, Version::V2, &limits).is_err());
        }
    }
}
