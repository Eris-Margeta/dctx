# 3-way benchmark headline (2026-10-30T09:17:03Z)

dctx `dctx-cli v0.25.0` sha256 `0a41481ffc6f`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 3.42 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.13s, 20/6 MB, 1.1 MB | 3.85s, 224/223 MB, 4.4 MB | 7.50s, 102/135 MB, 4.5 MB | dctx |
| ripgrep | 0.64s, 27/25 MB, 2.1 MB | 2.86s, 301/300 MB, 8.4 MB | 10.22s, 93/197 MB, 8.6 MB | dctx |
| pocketbase | 0.96s, 62/53 MB, 10.1 MB | 50.79s, 986/963 MB, 41.8 MB | 61.75s, 243/494 MB, 26.3 MB | dctx |
| trailcode | 1.09s, 88/76 MB, 15.0 MB | 56.56s, 1015/1014 MB, 41.1 MB | 49.55s, 392/383 MB, 11.9 MB | dctx |
| mixed | 0.29s, 73/17 MB, 3.8 MB | 3.63s, 450/420 MB, 12.8 MB | 4.43s, 73/124 MB, 2.2 MB | dctx |
| docs | 0.18s, 73/5 MB, 1.4 MB | 1.04s, 121/120 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 218 tok | 1066 ms, 262 tok | 552 ms, 1845 tok | dctx | dctx |
| ripgrep | 57 ms, 38 tok | 1169 ms, 392 tok | 717 ms, 3880 tok | dctx | dctx |
| pocketbase | 196 ms, 259 tok | 1498 ms, 358 tok | 1372 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 193 ms, 404 tok | 1807 ms, 624 tok | 1252 ms, 1865 tok | dctx | dctx |
| mixed | 116 ms, 189 tok | 1295 ms, 282 tok | 468 ms, 206 tok | dctx | dctx |
| docs | 86 ms, 359 tok | 952 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 45 ms, 479 tok | 980 ms, 136 tok | 345 ms, 32 tok | dctx | graphify |
| ripgrep | 56 ms, 484 tok | 984 ms, 801 tok | 408 ms, 29 tok | dctx | graphify |
| pocketbase | 194 ms, 464 tok | 1103 ms, 175 tok | 775 ms, 24 tok | dctx | graphify |
| trailcode | 188 ms, 472 tok | 1124 ms, 1064 tok | 713 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 48 ms, 866 tok | 969 ms, 852 tok | 490 ms, 474 tok | dctx | graphify |
| ripgrep | 58 ms, 1455 tok | 966 ms, 1125 tok | 647 ms, 533 tok | dctx | graphify |
| pocketbase | 226 ms, 383 tok | 1057 ms, 351 tok | 1266 ms, 813 tok | dctx | trail |
| trailcode | 189 ms, 784 tok | 1113 ms, 770 tok | 1116 ms, 759 tok | dctx | graphify |
| mixed | 139 ms, 1152 tok | 1008 ms, 1394 tok | 405 ms, 414 tok | dctx | graphify |
| docs | 86 ms, 933 tok | 942 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 493 tok | 975 ms, 1122 tok | 7 ms, 5930 tok | graphify | dctx |
| ripgrep | 59 ms, 576 tok | 1021 ms, 2538 tok | 7 ms, 7960 tok | graphify | dctx |
| pocketbase | 215 ms, 671 tok | 1109 ms, 1514 tok | 7 ms, 11591 tok | graphify | dctx |
| trailcode | 199 ms, 517 tok | 1129 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 148 ms, 611 tok | 1023 ms, 628 tok | 7 ms, 3827 tok | graphify | dctx |
| docs | 86 ms, 30 tok | 943 ms, 18 tok | unsupported | dctx | trail |

## Callers accuracy vs tool-independent regex gold (depth 1, ~20 targets per corpus; micro precision / recall)

| corpus | dctx P / R | Trail P / R | graphify P / R |
|---|---|---|---|
| flask | 1.000 / 0.977 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | 0.990 / 0.527 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | 1.000 / 0.815 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | 0.991 / 0.738 | 0.989 / 0.902 | 1.000 / 0.215 |
| ALL_GOLD_CORPORA | 0.995 / 0.725 | 0.986 / 0.436 | 0.989 / 0.196 |

dctx following all result pages (<=5): P 0.996 / R 0.982.

## Determinism / cache-friendliness

| corpus | tool | identical across 5 runs (queries) | identical after unrelated edit | ... ignoring dctx snapshot header line | changed after edit | first query after edit (s) | graphify `update` (s) |
|---|---|---|---|---|---|---|---|
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.123 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.327 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.550 | 5.31 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.531 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.785 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.777 | 8.11 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 1.096 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.661 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.412 | 57.64 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.117 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 3.910 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.369 | 40.94 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.302 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.224 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.562 | 4.50 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.166 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 0.935 | n/a |
| docs | graphify | 0/0 | 0/0 | 0/0 | - | n/a | n/a |

## Privacy canary (fixtures with .env, .env.local, settings.env, config/prod.env holding CANARY strings)

| fixture | tool | canary in any tool output | canary in index artifacts | queries that leaked |
|---|---|---|---|---|
| canary-tracked | dctx | False | False | - |
| canary-tracked | trail | False | False | - |
| canary-tracked | graphify | False | False | - |
| canary-ignored | dctx | False | False | - |
| canary-ignored | trail | False | False | - |
| canary-ignored | graphify | False | False | - |

## Network attempts (ptrace socket audit inside `unshare -Urn`, flask)

| tool | env variant | steps traced | AF_INET/INET6 connect/send attempts | targets (sample) | unix sockets |
|---|---|---|---|---|---|
| dctx | dnt1 | index_find, pack | 0 | - | - |
| trail | dnt1 | build, callers, ask | 0 | - | - |
| graphify | dnt1 | extract, query | 0 | - | - |
| dctx | default_env | index_find, pack | 0 | - | - |
| trail | default_env | build, callers, ask | 49 | connect 104.16.0.34:0 -> -Network is unreachable; connect 104.16.0.34:443 -> -Network is unreachable; connect 104.16.1.34:0 -> -Network is unreachable | /run/systemd/resolve/io.systemd.Resolve, /var/run/nscd/socket |
| graphify | default_env | extract, query | 0 | - | - |

## Language coverage on the mixed corpus (>=1 symbol / >=1 call edge from the 6 largest files per language)

| language | files | dctx sym/edge | Trail sym/edge | graphify sym/edge |
|---|---|---|---|---|
| scala | 2 | Y/Y (8/1) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | Y/Y (17/2) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/Y (12/1) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/Y (18/1) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/n (16/0) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/Y (25/2) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/Y (24/5) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/Y (22/4) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/Y (22/5) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/Y (8/2) | n/n (0/0) | Y/Y (42/23) |
