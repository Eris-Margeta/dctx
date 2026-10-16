//! Streaming SHA-512 for the RFC 8032 challenge hash, per FIPS 180-4 §5–6.
//! Owns a fixed-size block buffer and 128-bit bit count; no IO or allocation.
//! The verifier and public digest API share the private compression function.

use crate::compress::compress;

pub(crate) const INITIAL: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// Allocation-free SHA-512 state for messages shorter than 2^128 bits.
///
/// This is the ordinary SHA-512 digest, not an Ed25519ph interface.
#[derive(Clone)]
pub struct Sha512 {
    state: [u64; 8],
    buffer: [u8; 128],
    used: usize,
    bits: u128,
}

impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha512 {
    /// Creates an empty SHA-512 state.
    pub const fn new() -> Self {
        Self {
            state: INITIAL,
            buffer: [0; 128],
            used: 0,
            bits: 0,
        }
    }

    /// Appends bytes; empty updates are allowed and chunking does not affect the digest.
    pub fn update(&mut self, mut bytes: &[u8]) {
        self.bits = self.bits.wrapping_add((bytes.len() as u128) * 8);
        if self.used != 0 {
            let take = (128 - self.used).min(bytes.len());
            self.buffer[self.used..self.used + take].copy_from_slice(&bytes[..take]);
            self.used += take;
            bytes = &bytes[take..];
            if self.used < 128 {
                return;
            }
            compress(&mut self.state, &self.buffer);
            self.used = 0;
        }
        let (chunks, remainder) = bytes.as_chunks::<128>();
        for chunk in chunks {
            compress(&mut self.state, chunk);
        }
        self.buffer[..remainder.len()].copy_from_slice(remainder);
        self.used = remainder.len();
    }

    /// Consumes the state, adds FIPS padding and returns 64 digest bytes.
    pub fn finalize(mut self) -> [u8; 64] {
        self.buffer[self.used] = 0x80;
        self.buffer[self.used + 1..].fill(0);
        if self.used >= 112 {
            compress(&mut self.state, &self.buffer);
            self.buffer.fill(0);
        }
        self.buffer[112..].copy_from_slice(&self.bits.to_be_bytes());
        compress(&mut self.state, &self.buffer);
        let mut digest = [0; 64];
        for (chunk, word) in digest.as_chunks_mut::<8>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// Hashes a message in one call.
    pub fn digest(bytes: &[u8]) -> [u8; 64] {
        let mut hash = Self::new();
        hash.update(bytes);
        hash.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nist_known_answers() {
        for (message, expected) in [
            (
                b"".as_slice(),
                concat!(
                    "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce",
                    "47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
                ),
            ),
            (
                b"abc".as_slice(),
                concat!(
                    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
                    "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
                ),
            ),
            (
                concat!(
                    "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn",
                    "hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
                )
                .as_bytes(),
                concat!(
                    "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018",
                    "501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
                ),
            ),
        ] {
            assert_eq!(hex::encode(Sha512::digest(message)), expected);
        }
        let mut million = Sha512::new();
        for _ in 0..1000 {
            million.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex::encode(million.finalize()),
            concat!(
                "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb",
                "de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
            )
        );
    }

    #[test]
    fn count_carries_across_low_word() {
        let mut hash = Sha512::new();
        hash.bits = u128::from(u64::MAX) - 7;
        hash.update(&[0]);
        assert_eq!(hash.bits, 1u128 << 64);
    }
}
