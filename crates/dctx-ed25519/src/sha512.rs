// SPDX-License-Identifier: Apache-2.0
//! Streaming SHA-512 for the public digest API, per FIPS 180-4 §5–6.
//! Owns a fixed-size block buffer and 128-bit bit count; no IO or allocation.
//! Signature verification hashes inside ed25519-dalek, not through this type.

use crate::compress::compress;

pub(crate) const INITIAL: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// Allocation-free SHA-512 state for messages shorter than 2^128 bits.
///
/// This is the ordinary SHA-512 digest, not an Ed25519ph interface.
/// Messages are arbitrary bytes; there is no text decoding or normalization.
/// Clones are independent snapshots and may be updated on separate threads.
/// At most `u128::MAX / 8` message bytes may be appended to one state.
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
    /// Equivalent to [`Self::default`]; finalizing immediately hashes the empty message.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the public constructor's existing caller lint behavior."
    )]
    pub const fn new() -> Self {
        Self {
            state: INITIAL,
            buffer: [0; 128],
            used: 0,
            bits: 0,
        }
    }

    /// Appends bytes; empty updates are allowed and chunking does not affect the digest.
    ///
    /// Repeating a nonempty update appends those bytes again, in order.
    ///
    /// # Panics
    /// Panics before changing the state if the cumulative message would exceed
    /// `u128::MAX / 8` bytes (the FIPS 180-4 limit of fewer than 2^128 bits).
    /// Hash fewer bytes per message to stay within this limit.
    pub fn update(&mut self, mut bytes: &[u8]) {
        self.bits = self.bits.checked_add((bytes.len() as u128) * 8).expect(
            "SHA-512 message must be shorter than 2^128 bits; hash fewer bytes per message",
        );
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
    /// Clone the state first to finalize a snapshot while keeping the original
    /// available for further updates. Repeated snapshots produce identical bytes.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the consuming digest API's existing caller lint behavior."
    )]
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
    /// Equivalent to [`Self::new`], [`Self::update`] and [`Self::finalize`],
    /// including for empty input and arbitrary binary data.
    ///
    /// # Examples
    /// ```
    /// use dctx_ed25519::Sha512;
    /// let mut hash = Sha512::new();
    /// hash.update(b"a");
    /// let mut fork = hash.clone();
    /// fork.update(b"bc");
    /// assert_eq!(fork.finalize(), Sha512::digest(b"abc"));
    /// assert_eq!(hash.finalize(), Sha512::digest(b"a"));
    /// ```
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the one-shot digest API's existing caller lint behavior."
    )]
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

    #[test]
    fn maximum_length_is_accepted_and_overflow_leaves_state_unchanged() {
        // Inject only the unreachable-in-practice length; never forge a huge slice.
        let mut hash = Sha512::new();
        hash.bits = u128::MAX - 15;
        hash.used = 126;
        hash.update(&[0xff]);
        assert_eq!(hash.bits, u128::MAX - 7);
        assert_eq!(hash.used, 127);
        hash.update(&[]);
        let before = hash.clone();

        let overflow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hash.update(&[0]);
        }));
        let panic = overflow.expect_err("overlong SHA-512 input must not wrap");
        let message = panic
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
            .unwrap();
        assert!(message.contains("hash fewer bytes"));
        assert_eq!(hash.bits, before.bits);
        assert_eq!(hash.used, before.used);
        assert_eq!(hash.buffer, before.buffer);
        assert_eq!(hash.state, before.state);
        assert_eq!(hash.finalize(), before.finalize());
    }
}
