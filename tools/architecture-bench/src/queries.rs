//! The thirty warm `find` terms for the DX-A1 baseline.
//! Implements ARCHITECTURE-PROPOSAL §3 warm-query sample size.
//! Depends on nothing; the manifest check and the measure adapter share this list.

/// Fixed warm queries. Order is part of the reproducible sample.
pub const WARM_QUERIES: [&str; 30] = [
    "map", "index", "parse", "file", "node", "edge", "test", "pub", "use", "impl", "type",
    "const", "read", "write", "cache", "query", "scan", "store", "resolve", "ignore", "path",
    "hash", "source", "answer", "record", "budget", "place", "root", "symbol", "module",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_sample_has_thirty_plain_distinct_terms() {
        assert_eq!(WARM_QUERIES.len(), 30);
        let mut sorted = WARM_QUERIES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 30);
        assert!(WARM_QUERIES.iter().all(|term| {
            !term.is_empty() && !term.chars().any(char::is_control) && !term.contains(' ')
        }));
    }
}
