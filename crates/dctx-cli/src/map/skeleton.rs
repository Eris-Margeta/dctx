// SPDX-License-Identifier: Apache-2.0
//! `dctx map skeleton <file>`: the CLI side of the skeleton verb.
//! It parses as a map request (target required, `m:<path>` or a file), then turns the facade request into the skeleton verb.
//! The facade default of 2,000 tokens applies unless `--budget` is given; depth does not apply.

use dctx::{MapRequest, Verb};

/// Switches `mapped` to the skeleton verb: no depth, and the facade budget unless the caller set one.
pub(super) fn retarget(mapped: &mut MapRequest, budget_set: bool) {
    mapped.verb = Verb::Skeleton;
    mapped.depth = None;
    mapped.limits.depth = None;
    if !budget_set {
        mapped.limits.estimated_tokens = None;
        mapped.limits.encoded_bytes = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{args, facade_request};
    use super::*;

    #[test]
    fn skeleton_uses_the_facade_budget_and_no_depth() {
        let plain = args::parse(&["skeleton".into(), "m:a.py".into()]).expect("parse");
        let mut mapped = facade_request(&plain.request, plain.format, plain.continuation, plain.budget_set);
        retarget(&mut mapped, plain.budget_set);
        assert_eq!(mapped.verb, Verb::Skeleton);
        assert_eq!(mapped.target, "m:a.py");
        assert!(mapped.depth.is_none() && mapped.limits.depth.is_none());
        assert!(mapped.limits.estimated_tokens.is_none() && mapped.limits.encoded_bytes.is_none());

        let sized = args::parse(&["skeleton".into(), "m:a.py".into(), "--budget".into(), "300".into()]).expect("sized");
        let mut mapped = facade_request(&sized.request, sized.format, sized.continuation, sized.budget_set);
        retarget(&mut mapped, sized.budget_set);
        assert_eq!(mapped.limits.estimated_tokens, Some(300));
    }
}
