//! Rank statistics and cache-density arithmetic for DX-A1.
//! Implements ARCHITECTURE-PROPOSAL §3 measurement definitions for Stage 0.
//! Depends only on the bench error type; the measure adapter and report use it.

use crate::error::BenchError;

/// Nearest-rank percentile. Rank is `ceil(percent * n / 100)` on sorted samples.
pub fn percentile(samples: &[u64], percent: u32) -> Result<u64, BenchError> {
    if samples.is_empty() || percent == 0 || percent > 100 {
        return Err(BenchError::Message(
            "percentile needs a non-empty sample and percent 1..=100".into(),
        ));
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len() as u64;
    let rank = (u64::from(percent) * n + 99) / 100;
    let index = rank.max(1) - 1;
    Ok(sorted[index as usize])
}

/// `cache_bytes * 1000 / included_files`, refused as `None` when no files were indexed.
pub fn cache_bytes_per_thousand(cache_bytes: u64, files: u64) -> Option<u64> {
    if files == 0 {
        None
    } else {
        Some(cache_bytes.saturating_mul(1000) / files)
    }
}

/// Parses `VmHWM` kilobytes from `/proc/self/status` into bytes.
pub fn rss_from_proc_status(text: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

/// Parses `ps -o rss=` kilobytes into bytes.
pub fn rss_from_ps(text: &str) -> Option<u64> {
    let kb = text.split_whitespace().next()?.parse::<u64>().ok()?;
    Some(kb.saturating_mul(1024))
}

/// Microseconds, saturated rather than panicking on an extreme duration.
pub fn microseconds(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_refuses_empty_and_ranks_a_known_sample() {
        assert!(percentile(&[], 50).is_err());
        assert!(percentile(&[1], 0).is_err());
        assert!(percentile(&[1], 101).is_err());
        let samples: Vec<u64> = (1..=30).collect();
        assert_eq!(percentile(&samples, 50).unwrap(), 15);
        assert_eq!(percentile(&samples, 95).unwrap(), 29);
        assert_eq!(percentile(&samples, 100).unwrap(), 30);
        assert_eq!(percentile(&[8], 50).unwrap(), 8);
        assert!(percentile(&samples, 50).unwrap() <= percentile(&samples, 95).unwrap());
    }

    #[test]
    fn cache_density_refuses_zero_files_and_scales_the_rest() {
        assert_eq!(cache_bytes_per_thousand(2000, 0), None);
        assert_eq!(cache_bytes_per_thousand(2000, 2), Some(1_000_000));
        let files = [1_u64, 2, 10, 1000];
        for count in files {
            let cache = 4096_u64;
            let per = cache_bytes_per_thousand(cache, count).unwrap();
            assert!(per.saturating_mul(count) / 1000 <= cache);
        }
    }

    #[test]
    fn rss_parsers_read_kilobytes_and_refuse_blank_input() {
        assert_eq!(rss_from_proc_status("VmRSS:\t1 kB\n"), None);
        assert_eq!(
            rss_from_proc_status("Name:\tbench\nVmHWM:\t   123 kB\n"),
            Some(123 * 1024)
        );
        assert_eq!(rss_from_ps("   10\n"), Some(10 * 1024));
        assert_eq!(rss_from_ps("nope"), None);
    }
}
