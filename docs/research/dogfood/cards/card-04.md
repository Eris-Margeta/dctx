# Card 4: determinism, no-network and perf gates in CI

Files: `scripts/gate.sh`, `.github/workflows/gate.yml` (or runner script). Gate: two indexes → identical sha256; strace network count 0; cold-index and warm-query budgets asserted against `bench/results/latest.json` (cold index ≤ 2 s / ≤ 100 MB RSS, warm query p50 ≤ 300 ms).
