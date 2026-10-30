//! The total-source ceiling of one snapshot: a 1 GiB default that `DCTX_MAX_SOURCE_BYTES` can move.
//! Files are read one at a time and dropped, so the ceiling bounds work and disk reads, not resident memory.
//! Depends on [`super::error::DctxError`]. A smaller value still refuses a bigger root with `ResourceLimit`.

use super::error::DctxError;

/// Environment variable that sets the total-source ceiling, in decimal bytes.
pub const SOURCE_ENV: &str = "DCTX_MAX_SOURCE_BYTES";
/// Total content bytes one snapshot may read when `DCTX_MAX_SOURCE_BYTES` is unset: 1 GiB.
pub(crate) const SAFETY_SOURCE: u64 = 1024 * 1024 * 1024;
/// Largest value `DCTX_MAX_SOURCE_BYTES` may carry: 16 GiB.
const SOURCE_ENV_MAX: u64 = 16 * 1024 * 1024 * 1024;

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
