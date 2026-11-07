// SPDX-License-Identifier: Apache-2.0
//! The page size of `dctx map map <file>`: a file outline is listed to its record limit.
//! The query default of 500 tokens cuts a 40-record outline near record 20, so a plain map of an `m:` target
//! without `--budget` gets [`FILE_MAP_TOKENS`]. Skeleton, hubs, path, export and every other root keep their budgets.

use dctx_map_query::{AnswerBudget, Verb as QueryVerb};

use super::args::Options;

/// Default token budget of `map` on a file (`m:` target) without `--budget`; other roots keep the query default of 500.
pub(super) const FILE_MAP_TOKENS: u16 = 1000;

/// Gives a plain `map` of a file without `--budget` the [`FILE_MAP_TOKENS`] page; anything else is left as parsed.
pub(super) fn widen(options: &mut Options) -> Result<(), String> {
    let plain = options.request.verb == QueryVerb::Map
        && !options.skeleton
        && !options.hubs
        && options.export.is_none()
        && options.to.is_none();
    if plain
        && !options.budget_set
        && options.request.target.starts_with("m:")
        && options.request.budget.tokens() < FILE_MAP_TOKENS
    {
        options.request.budget =
            AnswerBudget::new(FILE_MAP_TOKENS).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::args;
    use super::*;

    #[test]
    fn a_file_map_without_budget_gets_the_file_page_and_other_roots_do_not() {
        let mut file = args::parse(&["map".into(), "m:a.rs".into(), "--depth".into(), "2".into()])
            .expect("map");
        widen(&mut file).expect("widen");
        assert_eq!(file.request.budget.tokens(), FILE_MAP_TOKENS);
        let mut sized = args::parse(&[
            "map".into(),
            "m:a.rs".into(),
            "--budget".into(),
            "300".into(),
        ])
        .expect("sized");
        widen(&mut sized).expect("widen");
        assert_eq!(sized.request.budget.tokens(), 300);
        let mut symbol = args::parse(&["map".into(), "s:a.rs#A".into()]).expect("symbol");
        widen(&mut symbol).expect("widen");
        assert_eq!(symbol.request.budget.tokens(), 500);
        let mut skeleton = args::parse(&["skeleton".into(), "m:a.rs".into()]).expect("skeleton");
        let before = skeleton.request.budget.tokens();
        widen(&mut skeleton).expect("widen");
        assert_eq!(skeleton.request.budget.tokens(), before);
        let mut find = args::parse(&["find".into(), "m:a.rs".into()]).expect("find");
        widen(&mut find).expect("widen");
        assert_eq!(find.request.budget.tokens(), 500);
    }
}
