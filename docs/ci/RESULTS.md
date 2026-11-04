<!-- SPDX-License-Identifier: Apache-2.0 -->
# CI results (self-hosted)

![ci](badge.svg)

The job in `.github/workflows/ci.yml` run step by step on a self-hosted Linux runner (16 cores) from a clean
`git archive` of the commit.

| Commit | Date (UTC) | Toolchain | Steps | Tests | Verdict |
|---|---|---|---|---|---|
| `79a36e3dca` | 2026-11-04 | rustc 1.99.0 | toolchain, licence headers, third-party licences, clippy, tests, release build, release gates | 2279 run, 2279 passed, 0 skipped | PASS |
