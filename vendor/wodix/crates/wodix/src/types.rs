//! Shared MDX/MDD metadata and physical record locations.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::{Error, Result};

/// Numeric layout used by the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// Version 1.x, with 32-bit format-sized numbers.
    V1,
    /// Version 2.x, with 64-bit format-sized numbers.
    V2,
}

impl TryFrom<f64> for Version {
    type Error = Error;

    fn try_from(value: f64) -> Result<Self> {
        if (1.0..2.0).contains(&value) {
            Ok(Self::V1)
        } else if (2.0..3.0).contains(&value) {
            Ok(Self::V2)
        } else {
            Err(Error::UnsupportedVersion(value))
        }
    }
}

impl Version {
    /// Size in bytes of a format-sized number: four for V1, eight for V2.
    pub fn number_size(self) -> usize {
        match self {
            Self::V1 => 4,
            Self::V2 => 8,
        }
    }
}

/// Text encodings supported by the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8.
    Utf8,
    /// Little-endian UTF-16, also used for MDD resource keys.
    Utf16Le,
    /// GB18030, including the supported GBK and GB2312 labels.
    Gb18030,
    /// Big5.
    Big5,
}

impl TryFrom<&str> for Encoding {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "UTF8" | "UTF-8" => Ok(Self::Utf8),
            "" | "UTF16" | "UTF-16" | "UTF16LE" | "UTF-16LE" => Ok(Self::Utf16Le),
            "GBK" | "GB2312" | "GB18030" | "GB-18030" => Ok(Self::Gb18030),
            "BIG5" | "BIG-5" => Ok(Self::Big5),
            _ => Err(Error::UnsupportedEncoding(value.to_owned())),
        }
    }
}

impl Encoding {
    /// Decode source bytes without replacement characters or BOM-based encoding changes.
    ///
    /// UTF-8 borrows the input when valid; other encodings may allocate.
    /// This does not trim NULs or whitespace, expand styles or follow redirects.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::EncodingError`] for invalid encoded text.
    pub fn decode<'a>(&self, data: &'a [u8]) -> Result<Cow<'a, str>> {
        if *self == Self::Utf8 {
            return std::str::from_utf8(data)
                .map(Cow::Borrowed)
                .map_err(|error| Error::EncodingError(format!("UTF-8: {error}")));
        }
        let encoding = match self {
            Self::Utf8 => unreachable!(),
            Self::Utf16Le => encoding_rs::UTF_16LE,
            Self::Gb18030 => encoding_rs::GB18030,
            Self::Big5 => encoding_rs::BIG5,
        };
        encoding
            .decode_without_bom_handling_and_without_replacement(data)
            .ok_or_else(|| Error::EncodingError(format!("invalid {} text", self.as_ref())))
    }
}

impl AsRef<str> for Encoding {
    fn as_ref(&self) -> &str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16Le => "utf-16le",
            Self::Gb18030 => "gb18030",
            Self::Big5 => "big5",
        }
    }
}

/// Source encryption bit flags; bit 1 marks encrypted key block metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encryption(
    /// Raw source flags: value `2` marks encrypted key-block metadata.
    pub u32,
);

impl Encryption {
    /// No source encryption.
    pub const NONE: Self = Self(0);
}

/// Style marker ID mapped to its opening and closing text.
///
/// Rules are applied by [`crate::Mdx::lookup`], not by [`crate::Read::text`].
pub type StyleSheet = BTreeMap<i32, (String, String)>;

/// Header metadata decoded from an MDX dictionary or MDD resource source.
#[derive(Debug, Clone)]
pub struct Metadata {
    /// Binary numeric layout used by the source.
    pub version: Version,
    /// Encoding used to decode keys and article text.
    pub encoding: Encoding,
    /// Validated source encryption flags.
    pub encrypted: Encryption,
    /// Dictionary title, or the caller's fallback if the source title is absent.
    pub title: String,
    /// Source description, which may contain markup.
    pub description: String,
    /// Source style-marker rules used by [`crate::Mdx::lookup`].
    pub stylesheets: StyleSheet,
    /// Whether the source requests right-to-left presentation.
    pub rtl: bool,
    /// All decoded XML header attributes, with original names and values.
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub compressed: u64,
    pub decompressed: u64,
}

/// A compressed block and its range in the logical decompressed stream.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start_pos: u64,
    pub end_pos: u64,
    pub shadow_start: u64,
    pub shadow_end: u64,
    pub compressed_size: u64,
    pub decompressed_size: u64,
}

impl Span {
    pub fn find(blocks: &[Self], offset: u64) -> Option<usize> {
        blocks
            .binary_search_by(|block| {
                if offset < block.shadow_start {
                    std::cmp::Ordering::Greater
                } else if offset >= block.shadow_end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()
    }
}

/// An entry's first block and length in the decompressed record stream.
/// The complete block directory is borrowed from its reader when accessed.
#[derive(Debug, Clone, Copy)]
pub struct Locator {
    pub compressed_block_pos: u64,
    pub record_offset: u64,
    pub record_size: u64,
}

/// A headword and its logical position in one source's record stream.
///
/// Its entry ID is its position in [`crate::Read::entries`], not this offset.
/// IDs are meaningful only for that exact source and its corresponding index;
/// duplicate headwords and aliases retain separate source-order IDs.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Byte offset in the logical decompressed record stream, not the source file.
    pub record_offset: u64,
    /// Original decoded headword, without index normalization.
    pub headword: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_reject_unsupported_layouts() {
        assert_eq!(Version::try_from(1.2).unwrap(), Version::V1);
        assert_eq!(Version::try_from(2.0).unwrap(), Version::V2);
        for value in [0.5, 3.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                Version::try_from(value),
                Err(Error::UnsupportedVersion(_))
            ));
        }
    }

    #[test]
    fn encoding_aliases_and_unknown_values() {
        for alias in ["GBK", "GB2312", "GB18030"] {
            assert_eq!(Encoding::try_from(alias).unwrap(), Encoding::Gb18030);
        }
        for alias in ["", "UTF-16", "UTF16LE"] {
            assert_eq!(Encoding::try_from(alias).unwrap(), Encoding::Utf16Le);
        }
        assert_eq!(Encoding::try_from("utf-8").unwrap(), Encoding::Utf8);
        assert!(matches!(
            Encoding::try_from("latin1"),
            Err(Error::UnsupportedEncoding(_))
        ));
    }

    #[test]
    fn strict_decoding_and_big5() {
        assert_eq!(
            Encoding::try_from("Big5")
                .unwrap()
                .decode(&[0xa4, 0xa4, 0xa4, 0xe5])
                .unwrap(),
            "中文"
        );
        assert_eq!(
            Encoding::Gb18030.decode(&[0xc4, 0xe3, 0xba, 0xc3]).unwrap(),
            "你好"
        );
        for (encoding, bytes) in [
            (Encoding::Utf8, &[0xff][..]),
            (Encoding::Utf16Le, &[0x00, 0xd8][..]),
            (Encoding::Utf16Le, &[0x61][..]),
            (Encoding::Gb18030, &[0x81][..]),
            (Encoding::Big5, &[0xa4][..]),
        ] {
            assert!(matches!(
                encoding.decode(bytes),
                Err(Error::EncodingError(_))
            ));
        }
    }

    #[test]
    fn record_index_find() {
        let blocks = [
            Span {
                start_pos: 0,
                end_pos: 100,
                shadow_start: 0,
                shadow_end: 500,
                compressed_size: 100,
                decompressed_size: 500,
            },
            Span {
                start_pos: 100,
                end_pos: 250,
                shadow_start: 500,
                shadow_end: 1200,
                compressed_size: 150,
                decompressed_size: 700,
            },
        ];

        assert_eq!(Span::find(&blocks, 0), Some(0));
        assert_eq!(Span::find(&blocks, 499), Some(0));
        assert_eq!(Span::find(&blocks, 500), Some(1));
        assert_eq!(Span::find(&blocks, 1199), Some(1));
        assert_eq!(Span::find(&blocks, 1200), None);
    }
}
