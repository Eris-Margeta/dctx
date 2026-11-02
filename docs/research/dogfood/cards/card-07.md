# Card 7: release docs generated from data

Files: `README.md` (template `README.tmpl`), `CHANGELOG.md`, `scripts/render_readme.py`, workspace `Cargo.toml` version 1.0.0. Gate: `make readme-check` fails if rendered README differs from committed; CHANGELOG has a `## 1.0.0` section listing out-of-scope items; `dctx --version` test.
