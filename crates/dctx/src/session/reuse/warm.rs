// SPDX-License-Identifier: Apache-2.0
//! A bounded decode cache, keyed by the entire verified retained object and decoder limit.
//! It changes no source checks, leases, rule checks, clock samples or encoded answer bytes.
use std::cell::RefCell;

use super::{DctxError, PersistedGeneration, decode_generation, records_cap};

const MAX_ENCODED: usize = 4 * 1024 * 1024;
const MAX_ROWS: usize = 50_000;

struct Cached {
    bytes: Vec<u8>,
    cap: usize,
    decoded: PersistedGeneration,
}

thread_local! {
    static CACHE: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

pub(super) fn decode(bytes: &[u8]) -> Result<PersistedGeneration, DctxError> {
    let cap = records_cap();
    CACHE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(cached) = slot.as_ref()
            && cached.cap == cap
            && cached.bytes == bytes
        {
            return Ok(cached.decoded.clone());
        }
        // Evict before decoding a different object, including a corrupt or oversized one.
        *slot = None;
        let decoded = decode_generation(bytes)?;
        let generation = &decoded.generation;
        let rows = generation
            .files
            .len()
            .saturating_add(generation.nodes.len())
            .saturating_add(generation.edges.len());
        if bytes.len() <= MAX_ENCODED && rows <= MAX_ROWS {
            *slot = Some(Cached {
                bytes: bytes.to_vec(),
                cap,
                decoded: decoded.clone(),
            });
        }
        Ok(decoded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FreshnessMode, ScanLimits};
    use crate::index::{Generation, IdIndex, TextBlob};

    fn encoded(source: &str) -> Vec<u8> {
        let generation = Generation {
            id: 1,
            digest: "digest".into(),
            encoded_len: 0,
            files: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            text: TextBlob::default(),
            freshness: FreshnessMode::Strict,
            taken_at: 0,
            epoch: 0,
            coverage: "full".into(),
            completion: "complete".into(),
            parser: "fixture".into(),
            ids: IdIndex::default(),
        };
        super::super::encode_generation(
            source,
            "default",
            &ScanLimits {
                entries: 100,
                depth: 10,
                file_bytes: 1000,
                source_bytes: 10000,
                records: 100,
            },
            &generation,
        )
        .expect("encode")
    }

    #[test]
    fn cached_indexes_are_independent_and_changed_or_corrupt_objects_cannot_hit() {
        let bytes = encoded("first");
        let original = decode_generation(&bytes).expect("uncached");
        let mut first = decode(&bytes).expect("first");
        assert_eq!(first.generation, original.generation);
        CACHE.with(|slot| assert_eq!(slot.borrow().as_ref().expect("retained").bytes, bytes));
        first.generation.digest = "caller mutation".into();
        assert_eq!(
            decode(&bytes).expect("warm").generation,
            original.generation
        );
        let changed = encoded("second");
        assert_eq!(decode(&changed).expect("changed").source_key, "second");
        let mut corrupt = changed;
        corrupt.push(0);
        assert!(decode(&corrupt).is_err());
        CACHE.with(|slot| assert!(slot.borrow().is_none()));
        assert_eq!(decode(&bytes).expect("recovered").source_key, "first");
    }
}
