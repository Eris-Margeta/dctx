<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# 3-way benchmark headline (2026-10-30T16:39:55Z)

dctx `dctx-cli v0.25.0` sha256 `76d8e9b59821`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 7.73 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.13s, 19/6 MB, 1.2 MB | 4.98s, 220/219 MB, 4.4 MB | 8.55s, 102/135 MB, 4.5 MB | dctx |
| ripgrep | 0.75s, 28/24 MB, 2.1 MB | 4.05s, 310/308 MB, 8.4 MB | 12.92s, 93/196 MB, 8.6 MB | dctx |
| pocketbase | 0.94s, 62/53 MB, 10.1 MB | 49.21s, 987/978 MB, 41.8 MB | 61.52s, 243/480 MB, 26.3 MB | dctx |
| trailcode | 1.09s, 90/81 MB, 15.0 MB | 55.47s, 1015/1006 MB, 41.1 MB | 43.28s, 393/386 MB, 11.9 MB | dctx |
| mixed | 0.35s, 74/21 MB, 5.3 MB | 3.76s, 472/463 MB, 12.8 MB | 4.25s, 74/124 MB, 2.2 MB | dctx |
| docs | 0.20s, 74/5 MB, 1.4 MB | 1.42s, 121/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 180 tok | 993 ms, 262 tok | 753 ms, 1845 tok | dctx | dctx |
| ripgrep | 81 ms, 31 tok | 1817 ms, 392 tok | 998 ms, 3880 tok | dctx | dctx |
| pocketbase | 195 ms, 207 tok | 1437 ms, 358 tok | 2135 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 188 ms, 346 tok | 1793 ms, 624 tok | 1222 ms, 1865 tok | dctx | dctx |
| mixed | 121 ms, 252 tok | 1229 ms, 282 tok | 471 ms, 206 tok | dctx | graphify |
| docs | 102 ms, 261 tok | 1634 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 45 ms, 482 tok | 977 ms, 136 tok | 447 ms, 32 tok | dctx | graphify |
| ripgrep | 85 ms, 500 tok | 1590 ms, 801 tok | 576 ms, 29 tok | dctx | graphify |
| pocketbase | 194 ms, 472 tok | 1058 ms, 175 tok | 1259 ms, 24 tok | dctx | graphify |
| trailcode | 190 ms, 478 tok | 1093 ms, 1064 tok | 693 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 866 tok | 974 ms, 852 tok | 627 ms, 474 tok | dctx | graphify |
| ripgrep | 81 ms, 1455 tok | 1578 ms, 1125 tok | 637 ms, 533 tok | dctx | graphify |
| pocketbase | 193 ms, 383 tok | 2032 ms, 351 tok | 1977 ms, 813 tok | dctx | trail |
| trailcode | 185 ms, 784 tok | 1083 ms, 770 tok | 1080 ms, 759 tok | dctx | graphify |
| mixed | 122 ms, 1152 tok | 1004 ms, 1394 tok | 409 ms, 414 tok | dctx | graphify |
| docs | 101 ms, 933 tok | 1606 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 494 tok | 987 ms, 1122 tok | 13 ms, 5930 tok | graphify | dctx |
| ripgrep | 84 ms, 576 tok | 1651 ms, 2538 tok | 7 ms, 7960 tok | graphify | dctx |
| pocketbase | 203 ms, 669 tok | 2165 ms, 1514 tok | 22 ms, 11591 tok | graphify | dctx |
| trailcode | 198 ms, 517 tok | 1108 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 125 ms, 570 tok | 1019 ms, 628 tok | 7 ms, 3827 tok | graphify | dctx |
| docs | 106 ms, 30 tok | 1639 ms, 18 tok | unsupported | dctx | trail |

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
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.200 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 2.075 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.833 | 7.36 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.512 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.806 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.750 | 7.86 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 1.277 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 5.351 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.483 | 54.65 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.152 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 3.966 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.243 | 37.02 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.415 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.237 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.712 | 6.52 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.226 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 1.681 | n/a |
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
| dart | 25 | Y/Y (110/2) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/Y (171/1) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/Y (194/1) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/Y (156/5) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/n (802/0) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/Y (453/5) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/Y (679/2) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/Y (533/5) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/n (374/0) | n/n (0/0) | Y/Y (42/23) |
