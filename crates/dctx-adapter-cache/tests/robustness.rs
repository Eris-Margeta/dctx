// SPDX-License-Identifier: Apache-2.0
//! Arbitrary and checksum-valid hostile cache ledgers must decode without panics.
#![forbid(unsafe_code)]

use dctx_adapter_cache::{Lifecycle, Quota, RootId, SessionId};
use dctx_hash::Sha256;
use proptest::prelude::*;

fn seed() -> Vec<u8> {
    let mut life = Lifecycle::new(Quota::scaled(4096)).unwrap();
    let root = RootId::new(1);
    life.register_root(root).unwrap();
    let fact = life.insert_fact(16, &[root]).unwrap();
    life.publish_view(root, 32, &[fact]).unwrap();
    let lease = life.reader_begin(root, SessionId::new(1)).unwrap();
    life.reader_confirm(lease).unwrap();
    life.encode_ledger().unwrap()
}

fn check(bytes: &[u8]) {
    match Lifecycle::decode_ledger(bytes) {
        Ok(life) => {
            assert!(life.p4_holds());
            let canonical = life.encode_ledger().unwrap();
            let restored = Lifecycle::decode_ledger(&canonical).unwrap();
            assert_eq!(restored.encode_ledger().unwrap(), canonical);
        }
        Err(error) => assert!(!error.to_string().is_empty()),
    }
}

proptest! {
    #[test]
    fn robustness_arbitrary_ledgers(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        check(&bytes);
        // Reach the body parser even when random bytes do not contain a header/checksum.
        let mut framed = b"DCTXLEDG\x01\0\0\0".to_vec();
        framed.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&bytes);
        let sum = Sha256::digest(&framed).unwrap();
        framed.extend_from_slice(sum.as_bytes());
        check(&framed);
    }

    #[test]
    fn robustness_checksum_valid_ledger_mutations(
        changes in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12),
        truncate in any::<prop::sample::Index>(),
    ) {
        let mut bytes = seed();
        for (index, value) in changes {
            let at = 16 + index.index(bytes.len() - 48);
            bytes[at] = value;
        }
        let end = bytes.len() - 32;
        let sum = Sha256::digest(&bytes[..end]).unwrap();
        bytes[end..].copy_from_slice(sum.as_bytes());
        check(&bytes);
        let cut = truncate.index(bytes.len());
        prop_assert!(Lifecycle::decode_ledger(&bytes[..cut]).is_err());
    }
}

#[test]
fn robustness_every_ledger_prefix_and_single_bit_corruption_is_refused() {
    let bytes = seed();
    check(&bytes);
    for cut in 0..bytes.len() {
        assert!(
            Lifecycle::decode_ledger(&bytes[..cut]).is_err(),
            "cut {cut}"
        );
    }
    for index in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[index] ^= 0x80;
        assert!(Lifecycle::decode_ledger(&corrupt).is_err(), "byte {index}");
    }
}
