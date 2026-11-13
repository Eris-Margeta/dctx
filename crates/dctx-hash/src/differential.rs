// SPDX-License-Identifier: Apache-2.0
//! Property test: streaming SHA-256 matches the `sha2` crate.
//! `sha2` is a dev-dependency only (enhanced-profile oracle). Not used in production.
//! Covers arbitrary bytes and an arbitrary split between two `update` calls.

#[cfg(test)]
mod tests {
    use crate::Sha256;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 128,
            failure_persistence: None,
            rng_seed: proptest::test_runner::RngSeed::Fixed(1113),
            rng_algorithm: proptest::test_runner::RngAlgorithm::ChaCha,
            ..ProptestConfig::default()
        })]

        #[test]
        fn matches_sha2_for_one_shot_and_split_updates(
            data in proptest::collection::vec(any::<u8>(), 0..2048),
            split in 0usize..4096,
        ) {
            let reference = <sha2::Sha256 as sha2::Digest>::digest(&data);
            let one_shot = Sha256::digest(&data).unwrap();
            prop_assert_eq!(one_shot.as_bytes().as_slice(), reference.as_slice());
            prop_assert_eq!(one_shot.byte_length(), data.len() as u64);

            let at = if data.is_empty() { 0 } else { split % data.len() };
            let mut hasher = Sha256::new();
            hasher.update(&data[..at]).unwrap();
            hasher.update(&data[at..]).unwrap();
            prop_assert_eq!(hasher.finalize(), one_shot);
        }
    }
}
