// SPDX-License-Identifier: Apache-2.0
//! Checksum-valid hostile ledgers are typed refusals, not debug panics.
//! Spec: CACHE-v1 ledger reconstruction; review F10.
//! Depends on the public ledger API and first-party dctx-hash.
//! Used by the dctx-adapter-cache regression suite.
//! A checksum does not prove quota words or ownership.

#[cfg(test)]
mod tests {
    use dctx_adapter_cache::{CacheError, Lifecycle, Quota, RootId};
    use dctx_hash::Sha256;

    fn word(bytes: &[u8], offset: usize) -> u64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[offset..offset + 8]);
        u64::from_le_bytes(buf)
    }

    fn resign(bytes: &mut [u8]) {
        let framed = bytes.len() - 32;
        let digest = Sha256::digest(&bytes[..framed]).expect("digest");
        bytes[framed..].copy_from_slice(digest.as_bytes());
    }

    #[test]
    fn checksum_valid_quota_words_are_corrupt_not_accepted() {
        let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
        let root = RootId::new(1);
        life.register_root(root).expect("root");
        let fact = life.insert_fact(10, &[root]).expect("fact");
        let valid = life.encode_ledger().expect("encode");
        assert_eq!(word(&valid, 16), 100);
        assert_eq!(word(&valid, 24), 75);
        assert_eq!(word(&valid, 32), 60);

        let restored = Lifecycle::decode_ledger(&valid).expect("control");
        assert_eq!(restored.object_bytes(fact), Some(10));
        assert_eq!(restored.admitted(), 10);
        assert!(restored.p4_holds());
        assert!(!restored.status().legacy_frozen);

        let mut hostile = valid;
        for offset in [16usize, 24, 32] {
            hostile[offset..offset + 8].copy_from_slice(&1u64.to_le_bytes());
        }
        resign(&mut hostile);
        let framed = hostile.len() - 32;
        let digest = Sha256::digest(&hostile[..framed]).expect("digest");
        assert_eq!(digest.as_bytes().as_slice(), &hostile[framed..]);
        assert!(matches!(
            Lifecycle::decode_ledger(&hostile),
            Err(CacheError::CorruptLedger { reason: "accounting" })
        ));
    }
}
