#![cfg(all(unix, target_pointer_width = "64"))]

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use wodix::{Error, Limits, Read, Reader};

struct Fixture;

impl Fixture {
    fn block(data: &[u8]) -> Vec<u8> {
        let mut bytes = 0_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&adler::adler32_slice(data).to_be_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    fn sparse(path: &Path) -> std::io::Result<u64> {
        const BLOCK: u64 = 16 * 1024 * 1024;
        const COUNT: u64 = 256;
        let large = BLOCK * COUNT;
        let xml = "<Dictionary GeneratedByEngineVersion=\"2.0\" Encoding=\"UTF-8\" Encrypted=\"0\" Title=\"Sparse offset regression\" Left2Right=\"Yes\" />";
        let text: Vec<u8> = xml
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut bytes = (text.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(&text);
        bytes.extend_from_slice(&adler::adler32_slice(&text).to_le_bytes());

        let mut keys = 0_u64.to_be_bytes().to_vec();
        keys.extend_from_slice(b"bulk\0");
        keys.extend_from_slice(&large.to_be_bytes());
        keys.extend_from_slice(b"tail\0");
        let encoded_keys = Self::block(&keys);
        let mut info = 2_u64.to_be_bytes().to_vec();
        for word in ["bulk", "tail"] {
            info.extend_from_slice(&(word.len() as u16).to_be_bytes());
            info.extend_from_slice(word.as_bytes());
            info.push(0);
        }
        info.extend_from_slice(&(encoded_keys.len() as u64).to_be_bytes());
        info.extend_from_slice(&(keys.len() as u64).to_be_bytes());
        let encoded_info = Self::block(&info);
        let mut key_header = Vec::new();
        for value in [
            1,
            2,
            info.len() as u64,
            encoded_info.len() as u64,
            encoded_keys.len() as u64,
        ] {
            key_header.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&key_header);
        bytes.extend_from_slice(&adler::adler32_slice(&key_header).to_be_bytes());
        bytes.extend_from_slice(&encoded_info);
        bytes.extend_from_slice(&encoded_keys);

        let tail = Self::block(b"TAIL\0");
        let physical = COUNT * (BLOCK + 8) + tail.len() as u64;
        for value in [COUNT + 1, 2, (COUNT + 1) * 16, physical] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for _ in 0..COUNT {
            bytes.extend_from_slice(&(BLOCK + 8).to_be_bytes());
            bytes.extend_from_slice(&BLOCK.to_be_bytes());
        }
        bytes.extend_from_slice(&(tail.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&5_u64.to_be_bytes());
        let offset = bytes.len() as u64;
        let mut file = File::create(path)?;
        file.write_all(&bytes)?;
        file.set_len(offset + physical)?;

        // Adler32 of n zero bytes: A=1, B=n mod 65521. Sparse holes are zeros.
        let checksum = (((BLOCK % 65521) << 16) | 1) as u32;
        for index in 0..COUNT {
            file.seek(SeekFrom::Start(offset + index * (BLOCK + 8)))?;
            file.write_all(&0_u32.to_be_bytes())?;
            file.write_all(&checksum.to_be_bytes())?;
        }
        file.seek(SeekFrom::Start(offset + COUNT * (BLOCK + 8)))?;
        file.write_all(&tail)?;
        file.sync_all()?;
        Ok(large)
    }
}

#[test]
fn record_beyond_four_gibibytes_uses_wide_offsets_and_explicit_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.mdx");
    let large = Fixture::sparse(&path).unwrap();
    let metadata = path.metadata().unwrap();
    assert!(metadata.len() > u32::MAX as u64);
    eprintln!(
        "large fixture: {} logical bytes, {} allocated bytes",
        metadata.len(),
        metadata.blocks() * 512
    );

    // SAFETY: The file is complete and is never modified during either mapping.
    let default = unsafe { Reader::open(&path) };
    assert!(matches!(default, Err(Error::Limit { .. })));

    let mut limits = Limits::default();
    limits.record = large as usize;
    // SAFETY: The fixture remains immutable until the reader is dropped.
    let reader = unsafe { Reader::open_with(&path, limits) }.unwrap();
    assert_eq!(reader.entries()[1].record_offset, large);
    assert_eq!(reader.text(1).unwrap(), "TAIL");
    assert_eq!(reader.read(1).unwrap().as_ref(), b"TAIL\0");
}
