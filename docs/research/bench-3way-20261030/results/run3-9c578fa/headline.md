# 3-way benchmark headline (2026-10-30T08:02:57Z)

dctx `dctx-cli v0.25.0` sha256 `72c6a31f0ee8`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 3.47 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.13s, 19/6 MB, 1.1 MB | 3.70s, 224/222 MB, 4.4 MB | 7.31s, 103/135 MB, 4.5 MB | dctx |
| ripgrep | 0.48s, 25/21 MB, 2.2 MB | 2.78s, 310/306 MB, 8.4 MB | 10.25s, 93/197 MB, 8.6 MB | dctx |
| pocketbase | 1.00s, 62/52 MB, 9.9 MB | 51.43s, 989/963 MB, 41.8 MB | 61.81s, 243/485 MB, 26.3 MB | dctx |
| trailcode | 1.31s, 88/77 MB, 14.8 MB | 63.33s, 1021/1019 MB, 41.1 MB | 43.60s, 392/389 MB, 11.9 MB | dctx |
| mixed | 0.28s, 74/12 MB, 3.4 MB | 3.56s, 453/447 MB, 12.8 MB | 4.39s, 74/122 MB, 2.2 MB | dctx |
| docs | 0.18s, 74/5 MB, 1.4 MB | 1.04s, 121/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 42 ms, 218 tok | 1009 ms, 262 tok | 544 ms, 1845 tok | dctx | dctx |
| ripgrep | 56 ms, 38 tok | 1150 ms, 392 tok | 716 ms, 3880 tok | dctx | dctx |
| pocketbase | 195 ms, 259 tok | 1905 ms, 358 tok | 1413 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 180 ms, 404 tok | 1723 ms, 624 tok | 1229 ms, 1865 tok | dctx | dctx |
| mixed | 113 ms, 199 tok | 1306 ms, 282 tok | 471 ms, 206 tok | dctx | dctx |
| docs | 86 ms, 359 tok | 952 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 479 tok | 957 ms, 136 tok | 342 ms, 32 tok | dctx | graphify |
| ripgrep | 55 ms, 484 tok | 969 ms, 801 tok | 403 ms, 29 tok | dctx | graphify |
| pocketbase | 197 ms, 464 tok | 1138 ms, 175 tok | 832 ms, 24 tok | dctx | graphify |
| trailcode | 180 ms, 472 tok | 1065 ms, 1064 tok | 683 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 42 ms, 866 tok | 968 ms, 852 tok | 483 ms, 474 tok | dctx | graphify |
| ripgrep | 55 ms, 1784 tok | 953 ms, 1125 tok | 638 ms, 533 tok | dctx | graphify |
| pocketbase | 282 ms, 383 tok | 1094 ms, 351 tok | 1291 ms, 813 tok | dctx | trail |
| trailcode | 177 ms, 784 tok | 1105 ms, 770 tok | 1117 ms, 759 tok | dctx | graphify |
| mixed | 112 ms, 1162 tok | 1018 ms, 1394 tok | 409 ms, 414 tok | dctx | graphify |
| docs | 87 ms, 933 tok | 945 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 491 tok | 968 ms, 1122 tok | 7 ms, 5930 tok | graphify | dctx |
| ripgrep | 57 ms, 590 tok | 1012 ms, 2538 tok | 8 ms, 7960 tok | graphify | dctx |
| pocketbase | 400 ms, 652 tok | 1147 ms, 1514 tok | 8 ms, 11591 tok | graphify | dctx |
| trailcode | 189 ms, 500 tok | 1165 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 116 ms, 603 tok | 1022 ms, 628 tok | 7 ms, 3827 tok | graphify | dctx |
| docs | 86 ms, 30 tok | 940 ms, 18 tok | unsupported | dctx | trail |

## Callers accuracy vs tool-independent regex gold (depth 1, ~20 targets per corpus; micro precision / recall)

| corpus | dctx P / R | Trail P / R | graphify P / R |
|---|---|---|---|
| flask | 1.000 / 0.836 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | 0.981 / 0.246 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | 1.000 / 0.667 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | 0.983 / 0.332 | 0.989 / 0.902 | 1.000 / 0.215 |
| ALL_GOLD_CORPORA | 0.993 / 0.461 | 0.986 / 0.436 | 0.989 / 0.196 |

dctx following all result pages (<=5): P 0.995 / R 0.706.

## Determinism / cache-friendliness

| corpus | tool | identical across 5 runs (queries) | identical after unrelated edit | ... ignoring dctx snapshot header line | changed after edit | first query after edit (s) | graphify `update` (s) |
|---|---|---|---|---|---|---|---|
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.122 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.310 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.563 | 5.39 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.478 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.809 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.736 | 8.01 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 0.997 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.757 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 2.145 | 63.25 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.138 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 4.255 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.316 | 38.30 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.292 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.223 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.558 | 4.41 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.172 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 0.946 | n/a |
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
| scala | 2 | Y/Y (5/1) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | Y/Y (16/3) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/Y (10/1) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/Y (17/1) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/Y (11/3) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/Y (25/2) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/Y (24/3) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/Y (22/4) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/Y (21/2) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/Y (8/2) | n/n (0/0) | Y/Y (42/23) |
