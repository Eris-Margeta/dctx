// SPDX-License-Identifier: Apache-2.0
//! Streaming and one-shot FIPS 180-4 SHA-256.
//! Implements the strict-profile content digest (decision 078, ARCHITECTURE-PROPOSAL).
//! Depends on `compress` and `algorithm`. Callers use [`Sha256`] from the crate root.
//! State is a fixed block buffer and an 8-word chain: `update` allocates nothing.

use crate::algorithm::ContentDigest;
use crate::compress::{compress, encode_state};
use crate::error::HashError;

/// Streaming SHA-256. `Copy` because the state is a fixed-size block, not a buffer on the heap.
///
/// Messages may contain arbitrary bytes, including invalid UTF-8. Updates append
/// bytes in order without normalization or deduplication. The total message must
/// contain at most `u64::MAX / 8` bytes. Copies are independent snapshots and can
/// be extended or finalized on separate threads.
#[derive(Clone, Copy, Debug)]
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    bytes: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// Starts a new SHA-256 computation (FIPS 180-4 §5.3.3).
    /// Equivalent to [`Self::default`]; finalizing immediately hashes the empty message.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buf: [0; 64],
            buf_len: 0,
            bytes: 0,
        }
    }

    /// Absorbs `data`. On [`HashError::MessageTooLong`] the hasher is unchanged.
    ///
    /// Empty updates are no-ops. Splitting the same message across updates does
    /// not change the digest; repeating a nonempty update appends those bytes again.
    ///
    /// # Errors
    /// Returns [`HashError::MessageTooLong`] if the total message would exceed
    /// the SHA-256 limit of `u64::MAX / 8` bytes. Hash fewer bytes in each message.
    pub fn update(&mut self, mut data: &[u8]) -> Result<(), HashError> {
        let additional = u64::try_from(data.len()).map_err(|_| HashError::MessageTooLong)?;
        let total = checked_message_len(self.bytes, additional)?;
        if self.buf_len > 0 {
            let space = 64 - self.buf_len;
            let take = space.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                compress(&mut self.state, &self.buf);
                self.buf_len = 0;
            }
        }
        while let Some((block, rest)) = data.split_first_chunk::<64>() {
            compress(&mut self.state, block);
            data = rest;
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
        self.bytes = total;
        Ok(())
    }

    /// Pads the message and returns the algorithm-tagged digest (FIPS 180-4 §5.1.1 and §5.2.1).
    ///
    /// Because the state is [`Copy`], this finalizes a snapshot: the original can
    /// still be updated or finalized again with byte-identical output.
    #[must_use]
    pub fn finalize(mut self) -> ContentDigest {
        let bit_len = bit_length(self.bytes);
        let content_bytes = self.bytes;
        self.buf[self.buf_len] = 0x80;
        let padded = self.buf_len + 1;
        if padded > 56 {
            self.buf[padded..].fill(0);
            compress(&mut self.state, &self.buf);
            self.buf = [0; 64];
        } else {
            self.buf[padded..56].fill(0);
        }
        self.buf[56..].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut self.state, &self.buf);
        ContentDigest::sha256(encode_state(&self.state), content_bytes)
    }

    /// Hashes `data` in one call. Same digest as `new`, `update`, `finalize`.
    ///
    /// # Errors
    /// Returns [`HashError::MessageTooLong`] if `data` exceeds the SHA-256
    /// limit of `u64::MAX / 8` bytes. Hash fewer bytes in each message.
    ///
    /// # Examples
    /// ```
    /// use dctx_hash::Sha256;
    /// let mut hash = Sha256::new();
    /// hash.update(b"a")?;
    /// hash.update(b"bc")?;
    /// assert_eq!(hash.finalize(), Sha256::digest(b"abc")?);
    /// # Ok::<(), dctx_hash::HashError>(())
    /// ```
    pub fn digest(data: &[u8]) -> Result<ContentDigest, HashError> {
        let mut hasher = Self::new();
        hasher.update(data)?;
        Ok(hasher.finalize())
    }
}

/// Accepts `additional` bytes only when the SHA-256 bit length still fits in `u64`.
fn checked_message_len(current: u64, additional: u64) -> Result<u64, HashError> {
    let total = current
        .checked_add(additional)
        .ok_or(HashError::MessageTooLong)?;
    if total.checked_mul(8).is_none() {
        return Err(HashError::MessageTooLong);
    }
    Ok(total)
}

/// Bit length of a message [`checked_message_len`] has already accepted.
/// That cap makes `bytes * 8` fit in `u64`, so wrapping equals the true product.
fn bit_length(bytes: u64) -> u64 {
    bytes.wrapping_mul(8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HashError;
    use crate::algorithm::{IDENTITY_VERSION, IdentityAlgorithm};

    fn hex(digest: &ContentDigest) -> String {
        use std::fmt::Write;

        let mut hex = String::with_capacity(64);
        for byte in digest.as_bytes() {
            write!(hex, "{byte:02x}").unwrap();
        }
        hex
    }

    fn assert_vector(message: &[u8], expected_hex: &str) {
        let digest = Sha256::digest(message).unwrap();
        assert_eq!(hex(&digest), expected_hex);
        assert_eq!(digest.byte_length(), message.len() as u64);
        assert_eq!(digest.algorithm(), IdentityAlgorithm::Sha256);
        assert_eq!(digest.version(), IDENTITY_VERSION);
        assert_eq!(digest.algorithm_code(), IdentityAlgorithm::SHA256);
    }

    /// FIPS 180-4 short messages: empty, one byte, and "abc" (one padded block).
    #[test]
    fn nist_short_message_vectors() {
        assert_vector(
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        assert_vector(
            b"a",
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
        );
        assert_vector(
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        );
    }

    /// FIPS 180-4 multi-block messages: the 448-bit example and the 896-bit example.
    #[test]
    fn nist_long_message_vectors() {
        assert_vector(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        );
        assert_vector(
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        );
    }

    /// NIST long vector: one million 0x61 bytes (FIPS 180-2 / CAVP).
    #[test]
    fn nist_million_a() {
        const EXPECTED: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
        let mut hasher = Sha256::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&chunk).unwrap();
        }
        let streamed = hasher.finalize();
        assert_eq!(hex(&streamed), EXPECTED);
        assert_eq!(streamed.byte_length(), 1_000_000);
        let one_shot = Sha256::digest(&vec![b'a'; 1_000_000]).unwrap();
        assert_eq!(streamed, one_shot);
    }

    #[test]
    fn incremental_equals_one_shot_across_block_edges() {
        let message = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        let one_shot = Sha256::digest(message).unwrap();
        for split in [0usize, 1, 55, 56, 63, 64, 65, message.len()] {
            let mut hasher = Sha256::new();
            hasher.update(&message[..split]).unwrap();
            hasher.update(&message[split..]).unwrap();
            assert_eq!(hasher.finalize(), one_shot);
        }
        let mut byte_at_a_time = Sha256::new();
        for byte in message {
            byte_at_a_time.update(&[*byte]).unwrap();
        }
        assert_eq!(byte_at_a_time.finalize(), one_shot);
        assert_eq!(Sha256::default().finalize(), Sha256::digest(b"").unwrap());
    }

    #[test]
    fn overlong_message_is_refused_and_a_legal_length_is_accepted() {
        let max_bytes = u64::MAX / 8;
        assert_eq!(checked_message_len(0, 0).unwrap(), 0);
        assert_eq!(checked_message_len(max_bytes, 0).unwrap(), max_bytes);
        assert_eq!(checked_message_len(max_bytes - 1, 1).unwrap(), max_bytes);
        assert_eq!(
            checked_message_len(max_bytes, 1),
            Err(HashError::MessageTooLong)
        );
        assert_eq!(
            checked_message_len(u64::MAX, 1),
            Err(HashError::MessageTooLong)
        );
        assert!(Sha256::digest(b"abc").is_ok());
    }

    #[test]
    fn state_is_a_fixed_size_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<Sha256>();
        assert!(core::mem::size_of::<Sha256>() <= 128);
        assert_eq!(bit_length(1), 8);
        assert_eq!(bit_length(u64::MAX / 8), u64::MAX - 7);
    }

    #[test]
    fn maximum_length_is_accepted_and_overflow_leaves_state_unchanged() {
        // Exercise the actual update guard without allocating an enormous message.
        let mut hasher = Sha256::new();
        hasher.bytes = u64::MAX / 8 - 1;
        hasher.buf_len = 62;
        hasher.update(&[0xff]).unwrap();
        assert_eq!(hasher.bytes, u64::MAX / 8);
        assert_eq!(hasher.buf_len, 63);
        hasher.update(&[]).unwrap();
        let before = hasher;
        for data in [&[0][..], &[0; 2], &[0; 64], &[0; 65]] {
            assert_eq!(hasher.update(data), Err(HashError::MessageTooLong));
            assert_eq!(hasher.bytes, before.bytes);
            assert_eq!(hasher.buf_len, before.buf_len);
            assert_eq!(hasher.buf, before.buf);
            assert_eq!(hasher.state, before.state);
            assert_eq!(hasher.finalize(), before.finalize());
        }
        hasher.update(&[]).unwrap();
        assert_eq!(hasher.finalize(), before.finalize());
    }
}
