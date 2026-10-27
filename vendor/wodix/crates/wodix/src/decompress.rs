//! Compressed block parsing and decompression.
//!
//! MDX block format: `[type: u32 BE] [checksum: u32 BE] [payload...]`
//!
//! Types:
//! - 0x00000000: uncompressed (Adler32 checksum on payload)
//! - 0x01000000: LZO1X compressed (Adler32 checksum on decompressed)
//! - 0x02000000: zlib (deflate) compressed (checks both zlib and block Adler32)

use crate::Result;
use crate::checksum::{Adler, Verify};
use crate::decode::{Cursor, Decode};
use crate::error::Error;
use crate::limits::Limits;

/// Compression type marker in the block header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    Lzo,
    Zlib,
}

impl TryFrom<u32> for CompressionType {
    type Error = Error;

    fn try_from(val: u32) -> Result<Self> {
        match val {
            0x0000_0000 => Ok(Self::None),
            0x0100_0000 => Ok(Self::Lzo),
            0x0200_0000 => Ok(Self::Zlib),
            other => Err(Error::DecompressFailed(format!(
                "unknown compression type: 0x{other:08X}"
            ))),
        }
    }
}

/// A complete compressed block, including its type and checksum header.
///
/// A nonzero `size` is exact; zero permits an unknown size bounded by `limit`.
pub struct Block {
    pub size: usize,
    pub limit: usize,
}

impl<'a> Decode<'a> for Block {
    type Output = Vec<u8>;

    fn decode(&self, compressed: &'a [u8]) -> Result<(Self::Output, usize)> {
        Limits::check("block", self.size as u64, self.limit)?;
        if compressed.len() < 8 {
            return Err(Error::DecompressFailed(
                "block too small (< 8 bytes)".into(),
            ));
        }

        let mut cursor = Cursor {
            data: compressed,
            position: 0,
        };
        let bytes = cursor.take(4)?;
        let compression = CompressionType::try_from(u32::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))?;
        let bytes = cursor.take(4)?;
        let checksum = Adler {
            expected: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        };
        let payload = cursor.take(compressed.len() - cursor.position)?;

        let output = match compression {
            CompressionType::None => {
                self.check_size(payload.len())?;
                payload.to_vec()
            }
            CompressionType::Lzo => self.lzo(payload)?,
            CompressionType::Zlib => self.zlib(payload)?,
        };
        self.check_size(output.len())?;
        checksum.verify(&output)?;
        Ok((output, cursor.position))
    }
}

impl Block {
    fn check_size(&self, actual: usize) -> Result<()> {
        Limits::check("block", actual as u64, self.limit)?;
        if self.size != 0 && self.size != actual {
            return Err(Error::DecompressFailed(format!(
                "expected {} bytes, got {actual}",
                self.size
            )));
        }
        Ok(())
    }

    fn exhausted(&self) -> Error {
        if self.size != 0 && self.size < self.limit {
            Error::DecompressFailed(format!("output exceeds declared size {}", self.size))
        } else {
            Error::Limit {
                kind: "block",
                actual: (self.limit as u64).saturating_add(1),
                limit: self.limit as u64,
            }
        }
    }

    fn zlib(&self, payload: &[u8]) -> Result<Vec<u8>> {
        use flate2::{Decompress, FlushDecompress, Status};

        let bound = if self.size == 0 {
            self.limit
        } else {
            self.size
        };
        let mut decoder = Decompress::new(true);
        let mut output = Vec::new();
        let initial = if self.size == 0 {
            bound.min(8192)
        } else {
            bound
        };
        output
            .try_reserve_exact(initial)
            .map_err(|error| Error::DecompressFailed(format!("zlib allocation: {error}")))?;
        loop {
            if output.len() == output.capacity() && output.len() < bound {
                let capacity = output.capacity().saturating_mul(2).max(8192).min(bound);
                output
                    .try_reserve_exact(capacity - output.len())
                    .map_err(|error| {
                        Error::DecompressFailed(format!("zlib allocation: {error}"))
                    })?;
            }
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let length = output.len();
            let available = (bound - length).min(output.capacity() - length);
            let status = decoder
                .decompress_uninit(
                    &payload[before_in as usize..],
                    &mut output.spare_capacity_mut()[..available],
                    FlushDecompress::None,
                )
                .map_err(|error| Error::DecompressFailed(format!("zlib: {error}")))?;
            let produced = usize::try_from(decoder.total_out() - before_out)
                .map_err(|_| Error::Corrupt("zlib output length overflow".into()))?;
            if produced > available {
                return Err(Error::Corrupt("zlib exceeded output slice".into()));
            }
            // SAFETY: flate2 initialized exactly `produced` bytes in the bounded
            // spare slice. Existing bytes remain initialized and length stays
            // within both capacity and the configured output bound.
            unsafe { output.set_len(length + produced) };
            if status == Status::StreamEnd {
                if decoder.total_in() != payload.len() as u64 {
                    return Err(Error::DecompressFailed("trailing zlib data".into()));
                }
                return Ok(output);
            }
            if decoder.total_in() == before_in && decoder.total_out() == before_out {
                return Err(if output.len() == bound {
                    self.exhausted()
                } else {
                    Error::DecompressFailed("truncated zlib stream".into())
                });
            }
        }
    }

    fn lzo(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let bound = if self.size == 0 {
            self.limit
        } else {
            self.size
        };
        let mut output = Vec::new();
        output
            .try_reserve_exact(bound)
            .map_err(|error| Error::DecompressFailed(format!("lzo allocation: {error}")))?;
        output.resize(bound, 0);
        let actual = lzokay::decompress::decompress(payload, &mut output).map_err(|error| {
            if error == lzokay::Error::OutputOverrun {
                self.exhausted()
            } else {
                Error::DecompressFailed(format!("lzo: {error}"))
            }
        })?;
        output.truncate(actual);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture;

    impl Fixture {
        /// Build a test compressed block with type + checksum + payload.
        fn block(comp_type: u32, payload: &[u8], checksum: u32) -> Vec<u8> {
            let mut buf = Vec::with_capacity(8 + payload.len());
            buf.extend_from_slice(&comp_type.to_be_bytes());
            buf.extend_from_slice(&checksum.to_be_bytes());
            buf.extend_from_slice(payload);
            buf
        }

        fn compressed(comp_type: u32, original: &[u8]) -> Vec<u8> {
            use std::io::Write;
            let payload = match comp_type {
                0 => original.to_vec(),
                0x0100_0000 => lzokay::compress::compress(original).unwrap(),
                0x0200_0000 => {
                    let mut encoder =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    encoder.write_all(original).unwrap();
                    encoder.finish().unwrap()
                }
                _ => unreachable!(),
            };
            Self::block(comp_type, &payload, adler::adler32_slice(original))
        }
    }

    #[test]
    fn decompress_uncompressed_with_adler32() {
        let payload = b"hello world";
        let checksum = adler::adler32_slice(payload);
        let block = Fixture::block(0x0000_0000, payload, checksum);

        let (result, consumed) = Block {
            size: 0,
            limit: 1024 * 1024,
        }
        .decode(&block)
        .unwrap();
        assert_eq!(result, payload);
        assert_eq!(consumed, block.len());
    }

    #[test]
    fn decompress_uncompressed_bad_checksum() {
        let payload = b"hello world";
        let block = Fixture::block(0x0000_0000, payload, 0xDEADBEEF);

        let result = Block {
            size: payload.len(),
            limit: 1024 * 1024,
        }
        .decode(&block);
        assert!(matches!(result, Err(Error::ChecksumMismatch)));
    }

    #[test]
    fn decompress_zlib_block() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let original = b"The quick brown fox jumps over the lazy dog";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed_payload = encoder.finish().unwrap();

        let checksum = adler::adler32_slice(original);
        let block = Fixture::block(0x0200_0000, &compressed_payload, checksum);

        let (result, consumed) = Block {
            size: 0,
            limit: 1024 * 1024,
        }
        .decode(&block)
        .unwrap();
        assert_eq!(result, original);
        assert_eq!(consumed, block.len());
    }

    #[test]
    fn decompress_zlib_bad_checksum() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let original = b"test data";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed_payload = encoder.finish().unwrap();

        let block = Fixture::block(0x0200_0000, &compressed_payload, 0xBADBAD);

        let result = Block {
            size: original.len(),
            limit: 1024 * 1024,
        }
        .decode(&block);
        assert!(matches!(result, Err(Error::ChecksumMismatch)));
    }

    #[test]
    fn decompress_unknown_type_errors() {
        let block = Fixture::block(0x0300_0000, b"data", 0);
        let result = Block {
            size: 4,
            limit: 1024 * 1024,
        }
        .decode(&block);
        assert!(matches!(result, Err(Error::DecompressFailed(_))));
    }

    #[test]
    fn decompress_too_small_block_errors() {
        let block = [0u8; 7];
        let result = Block {
            size: 0,
            limit: 1024 * 1024,
        }
        .decode(&block);
        assert!(matches!(result, Err(Error::DecompressFailed(_))));
    }

    #[test]
    fn decompress_lzo_block() {
        let original = b"LZO compressed test data that needs to be long enough for LZO to work well with compression";
        let compressed_payload = lzokay::compress::compress(original).unwrap();

        let checksum = adler::adler32_slice(original);
        let block = Fixture::block(0x0100_0000, &compressed_payload, checksum);

        let (result, consumed) = Block {
            size: original.len(),
            limit: 1024 * 1024,
        }
        .decode(&block)
        .unwrap();
        assert_eq!(result, original.as_slice());
        assert_eq!(consumed, block.len());
    }

    #[test]
    fn decompress_lzo_bad_checksum() {
        let original = b"LZO compressed test data that needs to be long enough for LZO to work well with compression";
        let compressed_payload = lzokay::compress::compress(original).unwrap();

        let block = Fixture::block(0x0100_0000, &compressed_payload, 0xDEAD);

        let result = Block {
            size: original.len(),
            limit: 1024 * 1024,
        }
        .decode(&block);
        assert!(matches!(result, Err(Error::ChecksumMismatch)));
    }

    #[test]
    fn all_codecs_require_exact_declared_sizes() {
        let original = b"exact decoded size matters even with a valid checksum";
        for compression in [0, 0x0100_0000, 0x0200_0000] {
            let data = Fixture::compressed(compression, original);
            for size in [original.len() - 1, original.len() + 1] {
                assert!(matches!(
                    Block { size, limit: 1024 }.decode(&data),
                    Err(Error::DecompressFailed(_))
                ));
            }
            assert_eq!(
                Block {
                    size: original.len(),
                    limit: original.len()
                }
                .decode(&data)
                .unwrap()
                .0,
                original
            );
            assert_eq!(
                Block {
                    size: 0,
                    limit: original.len()
                }
                .decode(&data)
                .unwrap()
                .0,
                original
            );
        }
    }

    #[test]
    fn all_codecs_enforce_unknown_output_limits() {
        let original = vec![b'a'; 32768];
        for compression in [0, 0x0100_0000, 0x0200_0000] {
            let data = Fixture::compressed(compression, &original);
            assert!(matches!(
                Block {
                    size: 0,
                    limit: 8192
                }
                .decode(&data),
                Err(Error::Limit { .. })
            ));
            assert!(matches!(
                Block {
                    size: usize::MAX,
                    limit: 8192
                }
                .decode(&data),
                Err(Error::Limit { .. })
            ));
        }
    }

    #[test]
    fn zlib_rejects_truncated_and_trailing_streams() {
        let original = b"complete stream";
        let mut data = Fixture::compressed(0x0200_0000, original);
        data.pop();
        assert!(matches!(
            Block {
                size: 0,
                limit: 1024
            }
            .decode(&data),
            Err(Error::DecompressFailed(_))
        ));
        let mut data = Fixture::compressed(0x0200_0000, original);
        data.push(0);
        assert!(matches!(
            Block {
                size: 0,
                limit: 1024
            }
            .decode(&data),
            Err(Error::DecompressFailed(_))
        ));
    }

    #[test]
    fn zlib_grows_direct_output_without_crossing_final_bound() {
        let original: Vec<u8> = (0..65537)
            .map(|index| ((index * 31) ^ (index >> 3)) as u8)
            .collect();
        let data = Fixture::compressed(0x0200_0000, &original);
        let output = Block {
            size: 0,
            limit: original.len(),
        }
        .decode(&data)
        .unwrap()
        .0;
        assert_eq!(output, original);
        assert!(matches!(
            Block {
                size: 0,
                limit: original.len() - 1
            }
            .decode(&data),
            Err(Error::Limit { .. })
        ));
    }
}
