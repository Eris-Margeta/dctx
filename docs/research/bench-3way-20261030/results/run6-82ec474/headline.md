<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# 3-way benchmark headline (2026-10-31T02:04:32Z)

dctx `dctx-cli v0.25.0` sha256 `bea5055d9878`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 1.49 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.18s, 20/6 MB, 1.2 MB | 3.73s, 224/222 MB, 4.4 MB | 7.83s, 102/135 MB, 4.5 MB | dctx |
| ripgrep | 0.61s, 28/23 MB, 2.1 MB | 3.25s, 300/298 MB, 8.4 MB | 12.86s, 93/198 MB, 8.6 MB | dctx |
| pocketbase | 0.96s, 63/51 MB, 10.1 MB | 52.45s, 987/963 MB, 41.8 MB | 59.02s, 243/476 MB, 26.3 MB | dctx |
| trailcode | 1.21s, 88/73 MB, 15.0 MB | 57.68s, 1021/1014 MB, 41.1 MB | 45.50s, 392/385 MB, 11.9 MB | dctx |
| mixed | 0.40s, 74/23 MB, 5.5 MB | 3.72s, 477/449 MB, 12.8 MB | 4.84s, 74/123 MB, 2.2 MB | dctx |
| docs | 0.20s, 74/6 MB, 1.4 MB | 1.16s, 121/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 51 ms, 180 tok | 1080 ms, 262 tok | 656 ms, 1845 tok | dctx | dctx |
| ripgrep | 81 ms, 31 tok | 1338 ms, 392 tok | 718 ms, 3880 tok | dctx | dctx |
| pocketbase | 192 ms, 207 tok | 1443 ms, 358 tok | 1467 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 206 ms, 346 tok | 1966 ms, 624 tok | 1234 ms, 1865 tok | dctx | dctx |
| mixed | 138 ms, 252 tok | 1371 ms, 282 tok | 480 ms, 206 tok | dctx | graphify |
| docs | 97 ms, 261 tok | 980 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 46 ms, 482 tok | 983 ms, 136 tok | 413 ms, 32 tok | dctx | graphify |
| ripgrep | 81 ms, 500 tok | 1138 ms, 801 tok | 427 ms, 29 tok | dctx | graphify |
| pocketbase | 193 ms, 472 tok | 1197 ms, 175 tok | 821 ms, 24 tok | dctx | graphify |
| trailcode | 213 ms, 478 tok | 1149 ms, 1064 tok | 708 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 47 ms, 866 tok | 962 ms, 852 tok | 636 ms, 474 tok | dctx | graphify |
| ripgrep | 62 ms, 1455 tok | 1103 ms, 1125 tok | 651 ms, 533 tok | dctx | graphify |
| pocketbase | 193 ms, 383 tok | 1088 ms, 351 tok | 1286 ms, 813 tok | dctx | trail |
| trailcode | 207 ms, 784 tok | 1206 ms, 770 tok | 1097 ms, 759 tok | dctx | graphify |
| mixed | 126 ms, 1152 tok | 1107 ms, 1394 tok | 421 ms, 414 tok | dctx | graphify |
| docs | 101 ms, 933 tok | 970 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 46 ms, 494 tok | 1008 ms, 1122 tok | 16 ms, 5930 tok | graphify | dctx |
| ripgrep | 64 ms, 576 tok | 1164 ms, 2538 tok | 8 ms, 7960 tok | graphify | dctx |
| pocketbase | 204 ms, 669 tok | 1157 ms, 1514 tok | 8 ms, 11591 tok | graphify | dctx |
| trailcode | 197 ms, 517 tok | 1157 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 154 ms, 586 tok | 1108 ms, 628 tok | 9 ms, 3827 tok | graphify | dctx |
| docs | 96 ms, 30 tok | 963 ms, 18 tok | unsupported | dctx | trail |

## Callers accuracy vs tool-independent regex gold (depth 1, ~20 targets per corpus; micro precision / recall)

| corpus | dctx P / R | Trail P / R | graphify P / R |
|---|---|---|---|
| flask | 1.000 / 0.977 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | 0.990 / 0.801 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | 1.000 / 0.842 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | 0.992 / 0.852 | 0.989 / 0.902 | 1.000 / 0.215 |
| ALL_GOLD_CORPORA | 0.995 / 0.851 | 0.986 / 0.436 | 0.989 / 0.196 |

dctx following all result pages (<=5): P 0.996 / R 0.968.

### Callers recall three ways: page-1 micro | all-pages micro | macro (mean of per-target recall, page-1 / all-pages)

Page 1 is one `neighbors --limit 40 --budget 2000` call (Trail and graphify answer in one complete listing, so for them all-pages equals page-1). Micro pools every gold site (a target with 137 sites outweighs one with 3); macro gives every target the same weight. dctx all-pages follows the cursor for at most 5 pages.

| corpus | gold sites | dctx p1 / all / macro | Trail p1 / all / macro | graphify p1 / all / macro |
|---|---|---|---|---|
| flask | 128 | 0.977 / 0.977 / 0.967 (0.967) | 0.328 / 0.328 / 0.750 (0.750) | 0.195 / 0.195 / 0.737 (0.737) |
| ripgrep | 281 | 0.801 / 0.940 / 0.756 (0.787) | 0.327 / 0.327 / 0.641 (0.641) | 0.032 / 0.032 / 0.319 (0.319) |
| pocketbase | 222 | 0.842 / 0.977 / 0.897 (0.925) | 0.099 / 0.099 / 0.507 (0.507) | 0.383 / 0.383 / 0.519 (0.519) |
| trailcode | 256 | 0.852 / 0.988 / 0.956 (0.992) | 0.902 / 0.902 / 0.758 (0.758) | 0.215 / 0.215 / 0.550 (0.550) |
| ALL_GOLD_CORPORA | 887 | 0.851 / 0.968 / 0.897 (0.920) | 0.436 / 0.436 / 0.672 (0.672) | 0.196 / 0.196 / 0.534 (0.534) |

## Determinism / cache-friendliness

| corpus | tool | identical across 5 runs (queries) | identical after unrelated edit | ... ignoring dctx snapshot header line | changed after edit | first query after edit (s) | graphify `update` (s) |
|---|---|---|---|---|---|---|---|
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.157 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.618 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.588 | 6.18 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.794 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 2.041 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.748 | 8.20 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 1.090 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.735 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.506 | 55.76 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.148 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 4.421 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.305 | 37.74 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.438 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.363 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.638 | 4.60 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.191 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 0.948 | n/a |
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
| trail | default_env | build, callers, ask | 1 | connect 54.185.153.58:443 -> -Network is unreachable | /run/systemd/resolve/io.systemd.Resolve, /var/run/nscd/socket |
| graphify | default_env | extract, query | 0 | - | - |

## Language coverage on the mixed corpus (>=1 symbol / >=1 call edge from the 6 largest files per language)

Symbol probe per tool: dctx `map skeleton FILE --json`; trail `skeleton FILE --json`; graphify `graph.json read directly`. Call-edge probe: dctx `map neighbors ID --dir both --edge calls` (first of 25 symbols with an edge), Trail `callers NAME --direction in|out`, graphify `calls` links in graph.json.

| language | files | dctx `map skeleton` sym/edge | Trail `skeleton` sym/edge | graphify graph.json sym/edge |
|---|---|---|---|---|
| scala | 2 | Y/Y (169/1) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | Y/Y (110/3) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/Y (171/1) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/Y (194/1) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/Y (156/5) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/Y (802/4) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/Y (453/5) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/Y (679/2) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/Y (533/5) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/n (374/0) | n/n (0/0) | Y/Y (42/23) |
