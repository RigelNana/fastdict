//! Borrowed decoding and checked sequential access to dictionary bytes.

use crate::Result;
use crate::error::Error;

/// Decode one value and report how many input bytes it consumed.
pub trait Decode<'a> {
    type Output;

    fn decode(&self, data: &'a [u8]) -> Result<(Self::Output, usize)>;
}

/// A position in borrowed input, advanced only after a successful read.
pub(crate) struct Cursor<'a> {
    pub data: &'a [u8],
    pub position: usize,
}

impl<'a> Cursor<'a> {
    pub fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| Error::Corrupt("byte range overflow".into()))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| Error::Corrupt("byte range exceeds input".into()))?;
        self.position = end;
        Ok(bytes)
    }

    pub fn read<D: Decode<'a>>(&mut self, decoder: &D) -> Result<D::Output> {
        let remaining = self
            .data
            .get(self.position..)
            .ok_or_else(|| Error::Corrupt("cursor position exceeds input".into()))?;
        let (value, consumed) = decoder.decode(remaining)?;
        if consumed > remaining.len() {
            return Err(Error::Corrupt("decoder consumed beyond input".into()));
        }
        self.position += consumed;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::number::Number;
    use crate::types::Version;

    struct Fixture {
        consumed: usize,
    }

    impl<'a> Decode<'a> for Fixture {
        type Output = &'a [u8];

        fn decode(&self, data: &'a [u8]) -> Result<(Self::Output, usize)> {
            Ok((data, self.consumed))
        }
    }

    #[test]
    fn reads_and_takes_advance_over_borrowed_input() {
        let data = [0, 0, 1, 0, 7, 8];
        let mut cursor = Cursor {
            data: &data,
            position: 0,
        };
        assert_eq!(
            cursor
                .read(&Number {
                    version: Version::V1
                })
                .unwrap(),
            256
        );
        assert_eq!(cursor.position, 4);
        let suffix = cursor.take(2).unwrap();
        assert_eq!(suffix, &[7, 8]);
        assert_eq!(suffix.as_ptr(), data[4..].as_ptr());
        assert_eq!(cursor.position, data.len());
        assert_eq!(cursor.take(0).unwrap(), &[]);
    }

    #[test]
    fn failed_reads_preserve_position() {
        let mut cursor = Cursor {
            data: &[0, 1, 2, 3],
            position: 1,
        };
        assert!(matches!(
            cursor.read(&Number {
                version: Version::V1
            }),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(cursor.position, 1);
        assert!(matches!(cursor.take(4), Err(Error::Corrupt(_))));
        assert_eq!(cursor.position, 1);
        assert!(matches!(cursor.take(usize::MAX), Err(Error::Corrupt(_))));
        assert_eq!(cursor.position, 1);
        assert_eq!(cursor.take(3).unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn decoder_consumption_is_checked_before_advancing() {
        let mut cursor = Cursor {
            data: &[1, 2, 3],
            position: 1,
        };
        assert!(matches!(
            cursor.read(&Fixture {
                consumed: usize::MAX
            }),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(cursor.position, 1);
        assert_eq!(cursor.read(&Fixture { consumed: 0 }).unwrap(), &[2, 3]);
        assert_eq!(cursor.position, 1);
        assert_eq!(cursor.read(&Fixture { consumed: 2 }).unwrap(), &[2, 3]);
        assert_eq!(cursor.position, 3);
    }

    #[test]
    fn invalid_initial_position_is_rejected() {
        let mut cursor = Cursor {
            data: &[1],
            position: 2,
        };
        assert!(matches!(cursor.take(0), Err(Error::Corrupt(_))));
        assert!(matches!(
            cursor.read(&Fixture { consumed: 0 }),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(cursor.position, 2);
    }
}
