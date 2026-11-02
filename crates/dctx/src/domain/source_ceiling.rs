//! The walk ceilings of one snapshot: 1 GiB of source (`DCTX_MAX_SOURCE_BYTES`), 1,000,000 records
//! (`DCTX_MAX_RECORDS`) and 100,000 entries (`DCTX_MAX_ENTRIES`). Each default can be moved by its variable.
//! Files are read one at a time and dropped, so the ceiling bounds work and disk reads, not resident memory.
//! Depends on [`super::error::DctxError`]. A smaller value still refuses a bigger root with `ResourceLimit`.

use super::error::DctxError;

/// Environment variable that sets the total-source ceiling, in decimal bytes.
pub const SOURCE_ENV: &str = "DCTX_MAX_SOURCE_BYTES";
/// Total content bytes one snapshot may read when `DCTX_MAX_SOURCE_BYTES` is unset: 1 GiB.
pub(crate) const SAFETY_SOURCE: u64 = 1024 * 1024 * 1024;
/// Largest value `DCTX_MAX_SOURCE_BYTES` may carry: 16 GiB.
const SOURCE_ENV_MAX: u64 = 16 * 1024 * 1024 * 1024;

/// Environment variable that sets the record ceiling (files, nodes and edges are each counted against it).
pub const RECORDS_ENV: &str = "DCTX_MAX_RECORDS";
/// Environment variable that sets the entry ceiling (files and directories one walk may visit).
pub const ENTRIES_ENV: &str = "DCTX_MAX_ENTRIES";
/// Records one snapshot may hold when `DCTX_MAX_RECORDS` is unset.
pub(crate) const SAFETY_RECORDS: u64 = 1_000_000;
/// Entries one snapshot may visit when `DCTX_MAX_ENTRIES` is unset.
pub(crate) const SAFETY_ENTRIES: u64 = 100_000;
/// Largest value `DCTX_MAX_RECORDS` and `DCTX_MAX_ENTRIES` may carry. Memory, not this number, is the real bound.
const COUNT_ENV_MAX: u64 = 100_000_000;

/// Parses a raw `DCTX_MAX_RECORDS` or `DCTX_MAX_ENTRIES` value: a decimal count in `1..=100_000_000`.
/// `None` is `default`.
///
/// # Errors
/// `InvalidRequest` naming the variable when the text is not a decimal number or is out of range.
pub fn parse_count_ceiling(name: &'static str, raw: Option<&str>, default: u64) -> Result<u64, DctxError> {
    let Some(text) = raw else { return Ok(default) };
    let reason = if name == RECORDS_ENV {
        "DCTX_MAX_RECORDS must be a decimal count from 1 to 100000000"
    } else {
        "DCTX_MAX_ENTRIES must be a decimal count from 1 to 100000000"
    };
    match text.trim().parse::<u64>() {
        Ok(value) if (1..=COUNT_ENV_MAX).contains(&value) => Ok(value),
        _ => Err(DctxError::invalid_detail(reason, text)),
    }
}

fn count_ceiling(name: &'static str, default: u64) -> Result<u64, DctxError> {
    match std::env::var(name) {
        Ok(text) => parse_count_ceiling(name, Some(&text), default),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(_)) => parse_count_ceiling(name, Some("not unicode"), default),
    }
}

/// The record ceiling now in force: `DCTX_MAX_RECORDS` when set, otherwise 1,000,000.
///
/// # Errors
/// `InvalidRequest` naming the variable when it is set to an invalid value.
pub fn records_ceiling() -> Result<u64, DctxError> {
    count_ceiling(RECORDS_ENV, SAFETY_RECORDS)
}

/// The entry ceiling now in force: `DCTX_MAX_ENTRIES` when set, otherwise 100,000.
///
/// # Errors
/// `InvalidRequest` naming the variable when it is set to an invalid value.
pub fn entries_ceiling() -> Result<u64, DctxError> {
    count_ceiling(ENTRIES_ENV, SAFETY_ENTRIES)
}

/// Parses a raw `DCTX_MAX_SOURCE_BYTES` value: decimal bytes in `1..=16 GiB`. `None` is the default.
///
/// # Errors
/// `InvalidRequest` naming the variable when the text is not a decimal number or is out of range.
pub fn parse_source_ceiling(raw: Option<&str>) -> Result<u64, DctxError> {
    let Some(text) = raw else { return Ok(SAFETY_SOURCE) };
    let reason = "DCTX_MAX_SOURCE_BYTES must be a decimal byte count from 1 to 17179869184";
    match text.trim().parse::<u64>() {
        Ok(value) if (1..=SOURCE_ENV_MAX).contains(&value) => Ok(value),
        _ => Err(DctxError::invalid_detail(reason, text)),
    }
}

/// The total-source ceiling now in force: `DCTX_MAX_SOURCE_BYTES` when set, otherwise 1 GiB.
///
/// # Errors
/// `InvalidRequest` naming the variable when it is set to an invalid value.
pub fn source_ceiling() -> Result<u64, DctxError> {
    match std::env::var(SOURCE_ENV) {
        Ok(text) => parse_source_ceiling(Some(&text)),
        Err(std::env::VarError::NotPresent) => Ok(SAFETY_SOURCE),
        Err(std::env::VarError::NotUnicode(_)) => parse_source_ceiling(Some("not unicode")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_ceilings_parse_the_default_valid_invalid_and_out_of_range() {
        assert_eq!(parse_count_ceiling(RECORDS_ENV, None, SAFETY_RECORDS).expect("default"), 1_000_000);
        assert_eq!(parse_count_ceiling(ENTRIES_ENV, Some("5000000"), SAFETY_ENTRIES).expect("raised"), 5_000_000);
        assert_eq!(parse_count_ceiling(ENTRIES_ENV, Some("1"), SAFETY_ENTRIES).expect("min"), 1);
        for bad in ["", "abc", "0", "-1", "100000001", "1.5"] {
            let error = parse_count_ceiling(RECORDS_ENV, Some(bad), SAFETY_RECORDS).expect_err(bad);
            assert_eq!(error.code().as_str(), "InvalidRequest", "{bad}");
            assert!(error.to_string().contains(RECORDS_ENV), "{bad}: {error}");
            let error = parse_count_ceiling(ENTRIES_ENV, Some(bad), SAFETY_ENTRIES).expect_err(bad);
            assert!(error.to_string().contains(ENTRIES_ENV), "{bad}: {error}");
        }
    }

    #[test]
    fn source_ceiling_parses_the_default_valid_invalid_and_out_of_range() {
        assert_eq!(parse_source_ceiling(None).expect("default"), 1024 * 1024 * 1024);
        assert_eq!(parse_source_ceiling(Some("1")).expect("min"), 1);
        assert_eq!(parse_source_ceiling(Some("1000000")).expect("small"), 1_000_000);
        assert_eq!(parse_source_ceiling(Some("17179869184")).expect("max"), 16 * 1024 * 1024 * 1024);
        for bad in ["", "abc", "1.5", "-4", "0", "17179869185", "99999999999999999999999"] {
            let error = parse_source_ceiling(Some(bad)).expect_err(bad);
            assert_eq!(error.code().as_str(), "InvalidRequest", "{bad}");
            assert!(error.to_string().contains(SOURCE_ENV), "{bad}: {error}");
        }
    }
}
