//! Filter selection for `dctx pack`.

use dctx_core::datatypes::IgnoreRule;
use dctx_core::filter::FilterConfig;
use dctx_core::selection::parse_ignore_pattern_line;

/// ADR-011 view plus include/exclude globs. Binary omission is the default.
pub(super) struct Filters {
    pub(super) config: FilterConfig,
    pub(super) include: Vec<IgnoreRule>,
    pub(super) exclude: Vec<IgnoreRule>,
    pub(super) include_patterns: Vec<String>,
    pub(super) exclude_patterns: Vec<String>,
    pub(super) debug_ignored: bool,
}

impl Filters {
    pub(super) fn new() -> Self {
        Self {
            config: FilterConfig::default(),
            include: Vec::new(),
            exclude: Vec::new(),
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            debug_ignored: false,
        }
    }

    /// True when the caller asked for a view the JSON summary cannot show.
    pub(super) fn affects_view(&self) -> bool {
        !self.config.extensions.is_empty()
            || self.config.include_binary
            || self.config.only_empty
            || self.config.only_non_empty
            || self.config.min_size_bytes.is_some()
            || self.config.max_size_bytes.is_some()
            || self.config.only_hidden
            || !self.include.is_empty()
            || !self.exclude.is_empty()
    }
}

pub(super) fn parse_extensions(value: &str) -> Result<Vec<String>, String> {
    let extensions: Vec<_> = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if extensions.is_empty() {
        return Err("--ext requires at least one extension".to_string());
    }
    Ok(extensions)
}

pub(super) fn parse_megabytes(flag: &str, value: &str) -> Result<f64, String> {
    let size = value
        .parse::<f64>()
        .map_err(|_| format!("Invalid size for {flag}: {value}"))?;
    if !size.is_finite() || size < 0.0 {
        return Err(format!("Invalid size for {flag}: {value}"));
    }
    Ok(size)
}

pub(super) fn parse_pattern(flag: &str, value: &str) -> Result<IgnoreRule, String> {
    let rule =
        parse_ignore_pattern_line(value).ok_or_else(|| format!("{flag} requires a pattern"))?;
    if rule.pattern.is_empty() {
        return Err(format!("{flag} requires a pattern"));
    }
    if rule.is_negation {
        return Err(format!("{flag} does not take a negated pattern"));
    }
    Ok(rule)
}
