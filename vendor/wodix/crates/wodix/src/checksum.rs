//! Adler-32 checksum verification for MDX blocks.

use crate::Result;
use crate::error::Error;

/// Verify the integrity of a byte slice.
pub trait Verify {
    fn verify(&self, data: &[u8]) -> Result<()>;
}

/// The expected Adler-32 checksum of an MDX block.
pub struct Adler {
    pub expected: u32,
}

impl Verify for Adler {
    fn verify(&self, data: &[u8]) -> Result<()> {
        if adler::adler32_slice(data) == self.expected {
            Ok(())
        } else {
            Err(Error::ChecksumMismatch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler32_valid() {
        let data = b"Hello, world!";
        let checksum = adler::adler32_slice(data);
        assert!(Adler { expected: checksum }.verify(data).is_ok());
    }

    #[test]
    fn adler32_invalid() {
        let data = b"Hello, world!";
        let bad_checksum = 0xDEADBEEF;
        assert!(matches!(
            Adler {
                expected: bad_checksum
            }
            .verify(data),
            Err(Error::ChecksumMismatch)
        ));
    }

    #[test]
    fn adler32_empty_input() {
        let data: &[u8] = &[];
        assert!(Adler { expected: 1 }.verify(data).is_ok());
    }

    #[test]
    fn adler32_known_value() {
        // "Wikipedia" has known Adler-32 = 0x11E60398
        let data = b"Wikipedia";
        assert!(
            Adler {
                expected: 0x11E60398
            }
            .verify(data)
            .is_ok()
        );
    }
}
