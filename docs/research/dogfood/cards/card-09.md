# Card 9: agent integration (dogfood)

Goal: cards are generated with dctx context. Files: `crates/cli/src/cmd/pack.rs` (add `--budget N --for "<text>"` that runs `find` + `neighbors` on terms in the card text and truncates to budget), `tools/cardgen/context.sh`, `AGENTS.md` snippet. Gate: test that `pack --budget 3000` never exceeds 3000 tokens and includes the card's named symbols; pipeline run on 10 real cards with before/after prompt tokens logged.
