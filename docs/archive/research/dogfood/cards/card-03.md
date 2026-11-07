Historical design record; superseded by [the current documentation](../../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Card 3: language registry and tier hygiene

Goal: claims match code. Files: `map-parse/src/languages.rs` (new registry), `crates/cli/src/cmd/languages.rs`, `Cargo.toml` (`default-members` excludes `lexer-perl/fortran/verilog`; feature `experimental-lexers`), shell marked *outline*. Gate: test that `dctx languages --json` equals README table and mixed-corpus run reports symbols/edges per the tier promised.
