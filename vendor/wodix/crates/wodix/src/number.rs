//! Versioned number reading for MDX binary format.
//!
//! v1.x uses big-endian u32; v2.0+ uses big-endian i64.

use crate::Result;
use crate::decode::{Cursor, Decode};
use crate::types::Version;

/// A version-dependent big-endian dictionary number.
pub struct Number {
    pub version: Version,
}

impl<'a> Decode<'a> for Number {
    type Output = u64;

    fn decode(&self, data: &'a [u8]) -> Result<(Self::Output, usize)> {
        let mut cursor = Cursor { data, position: 0 };
        let value = match self.version {
            Version::V1 => {
                let bytes = cursor.take(4)?;
                u64::from(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            Version::V2 => {
                let bytes = cursor.take(8)?;
                // Preserve all bits of the format's i64 field in the unsigned output.
                u64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            }
        };
        Ok((value, cursor.position))
    }
}

/// A text length encoded as u8 in v1 or big-endian u16 in v2.
pub struct TextLen {
    pub version: Version,
}

impl<'a> Decode<'a> for TextLen {
    type Output = u32;

    fn decode(&self, data: &'a [u8]) -> Result<(Self::Output, usize)> {
        let mut cursor = Cursor { data, position: 0 };
        let value = match self.version {
            Version::V1 => u32::from(cursor.take(1)?[0]),
            Version::V2 => {
                let bytes = cursor.take(2)?;
                u32::from(u16::from_be_bytes([bytes[0], bytes[1]]))
            }
        };
        Ok((value, cursor.position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u32_be_from_bytes() {
        let data = [0x00, 0x01, 0x00, 0x00]; // 65536
        let (val, consumed) = Number {
            version: Version::V1,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(val, 65536);
        assert_eq!(consumed, 4);
    }

    #[test]
    fn read_i64_be_from_bytes() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00]; // 65536
        let (val, consumed) = Number {
            version: Version::V2,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(val, 65536);
        assert_eq!(consumed, 8);
    }

    #[test]
    fn read_u32_max() {
        let data = 0xFFFF_FFFFu32.to_be_bytes();
        let (val, _) = Number {
            version: Version::V1,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(val, 0xFFFF_FFFF);
    }

    #[test]
    fn read_number_v1_too_short() {
        let data = [0x00, 0x01, 0x02];
        assert!(
            Number {
                version: Version::V1
            }
            .decode(&data)
            .is_err()
        );
    }

    #[test]
    fn read_number_v2_too_short() {
        let data = [0x00; 7];
        assert!(
            Number {
                version: Version::V2
            }
            .decode(&data)
            .is_err()
        );
    }

    #[test]
    fn read_u8_single_byte() {
        let data = [0x42];
        let (val, consumed) = TextLen {
            version: Version::V1,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(val, 0x42);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn read_u16_two_bytes() {
        let data = [0x01, 0x00]; // 256
        let (val, consumed) = TextLen {
            version: Version::V2,
        }
        .decode(&data)
        .unwrap();
        assert_eq!(val, 256);
        assert_eq!(consumed, 2);
    }

    #[test]
    fn read_u8_empty_errors() {
        let data: &[u8] = &[];
        assert!(
            TextLen {
                version: Version::V1
            }
            .decode(data)
            .is_err()
        );
    }

    #[test]
    fn read_u16_one_byte_errors() {
        let data = [0x01];
        assert!(
            TextLen {
                version: Version::V2
            }
            .decode(&data)
            .is_err()
        );
    }
}
