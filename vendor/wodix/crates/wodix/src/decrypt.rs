//! RIPEMD-128 based decryption for MDX Key Block Info.
//!
//! Algorithm (matching GoldenDict):
//! 1. `key = RIPEMD128(buffer[4..8] ++ b"\x95\x36\x00\x00")` → 16 bytes
//! 2. Starting from `buffer[8]`, for each byte `i`:
//!    - `rotated = (byte >> 4) | (byte << 4)`
//!    - `decoded = rotated ^ prev ^ (i & 0xFF) ^ key[i % 16]`
//!    - `prev` is the original byte before decoding
//!    - Initial prev = 0x36

use ripemd::{Digest, Ripemd128};

/// RIPEMD-128 based Key Block Info decryption.
pub struct Cipher;

impl Cipher {
    /// Decrypt in place, preserving the first eight bytes used for key derivation.
    pub fn apply(buffer: &mut [u8]) -> crate::Result<()> {
        if buffer.len() <= 8 {
            return Err(crate::Error::DecryptFailed);
        }

        // Derive key: RIPEMD128(buffer[4..8] + magic).
        let mut hasher = Ripemd128::new();
        hasher.update(&buffer[4..8]);
        hasher.update(b"\x95\x36\x00\x00");
        let key: [u8; 16] = hasher.finalize().into();

        let mut prev: u8 = 0x36;
        for (i, byte) in buffer[8..].iter_mut().enumerate() {
            let original = *byte;
            let rotated = original.rotate_left(4);
            *byte = rotated ^ prev ^ (i as u8) ^ key[i % 16];
            prev = original;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_roundtrip() {
        // Create a test buffer, encrypt it (reverse the algorithm), then decrypt
        let plaintext = b"Hello, world! This is a test buffer for decryption.";

        // Build buffer with 8-byte header + plaintext
        let mut buffer = vec![0u8; 8 + plaintext.len()];
        buffer[4] = 0xAA;
        buffer[5] = 0xBB;
        buffer[6] = 0xCC;
        buffer[7] = 0xDD;

        // Derive key for encryption (same as decryption)
        let mut hasher = Ripemd128::new();
        hasher.update(&buffer[4..8]);
        hasher.update(b"\x95\x36\x00\x00");
        let key: [u8; 16] = hasher.finalize().into();

        // "Encrypt" — reverse the decrypt operation
        let mut prev: u8 = 0x36;
        for (i, &plain_byte) in plaintext.iter().enumerate() {
            let xored = plain_byte ^ prev ^ (i as u8) ^ key[i % 16];
            let encrypted = xored.rotate_left(4);
            buffer[8 + i] = encrypted;
            prev = encrypted;
        }

        // Now decrypt and verify
        Cipher::apply(&mut buffer).unwrap();
        assert_eq!(&buffer[8..], plaintext);
        assert_eq!(&buffer[..8], &[0, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn decrypt_too_short_errors() {
        let mut buffer = [0u8; 8]; // exactly 8 bytes — no data to decrypt
        let result = Cipher::apply(&mut buffer);
        assert!(matches!(result, Err(crate::Error::DecryptFailed)));
    }
}
