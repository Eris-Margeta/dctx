<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# 3-way benchmark headline (2026-10-30T03:26:29Z)

dctx `dctx-cli v0.25.0` sha256 `db3d6739c935`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 8.07 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.13s, 20/5 MB, 0.8 MB | 3.74s, 221/220 MB, 4.4 MB | 7.70s, 103/135 MB, 4.5 MB | dctx |
| ripgrep | 0.54s, 32/18 MB, 1.8 MB | 3.27s, 303/302 MB, 8.4 MB | 11.42s, 93/194 MB, 8.6 MB | dctx |
| pocketbase | 0.73s, 44/30 MB, 6.7 MB | 51.41s, 987/964 MB, 41.8 MB | 58.48s, 243/482 MB, 26.3 MB | dctx |
| trailcode | 0.85s, 98/32 MB, 10.8 MB | 51.56s, 1018/1010 MB, 41.1 MB | 44.10s, 391/387 MB, 11.9 MB | dctx |
| mixed | 0.18s, 98/5 MB, 0.9 MB | 4.02s, 450/444 MB, 12.8 MB | 4.68s, 98/125 MB, 2.2 MB | dctx |
| docs | 0.18s, 98/4 MB, 0.9 MB | 1.20s, 121/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 243 tok | 1051 ms, 262 tok | 545 ms, 1845 tok | dctx | dctx |
| ripgrep | 57 ms, 62 tok | 1171 ms, 392 tok | 711 ms, 3880 tok | dctx | dctx |
| pocketbase | 193 ms, 284 tok | 1498 ms, 358 tok | 1383 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 181 ms, 429 tok | 1880 ms, 624 tok | 1207 ms, 1865 tok | dctx | dctx |
| mixed | 101 ms, 208 tok | 1391 ms, 282 tok | 486 ms, 206 tok | dctx | graphify |
| docs | 92 ms, 352 tok | 959 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 30 tok | 1013 ms, 136 tok | 341 ms, 32 tok | dctx | dctx |
| ripgrep | 58 ms, 31 tok | 993 ms, 801 tok | 407 ms, 29 tok | dctx | graphify |
| pocketbase | 192 ms, 32 tok | 1083 ms, 175 tok | 787 ms, 24 tok | dctx | graphify |
| trailcode | 185 ms, 32 tok | 1208 ms, 1064 tok | 673 ms, 196 tok | dctx | dctx |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 380 tok | 951 ms, 852 tok | 485 ms, 474 tok | dctx | dctx |
| ripgrep | 72 ms, 486 tok | 998 ms, 1125 tok | 645 ms, 533 tok | dctx | dctx |
| pocketbase | 195 ms, 311 tok | 1177 ms, 351 tok | 1322 ms, 813 tok | dctx | dctx |
| trailcode | 175 ms, 472 tok | 1185 ms, 770 tok | 1076 ms, 759 tok | dctx | dctx |
| mixed | 100 ms, 28 tok | 1195 ms, 1394 tok | 418 ms, 414 tok | dctx | dctx |
| docs | 92 ms, 28 tok | 954 ms, 29 tok | unsupported | dctx | dctx |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 86 ms, 453955 tok | 960 ms, 1122 tok | 7 ms, 5930 tok | graphify | trail |
| ripgrep | 191 ms, 878688 tok | 1018 ms, 2538 tok | 7 ms, 7960 tok | graphify | trail |
| pocketbase | 446 ms, 2275690 tok | 1120 ms, 1514 tok | 8 ms, 11591 tok | graphify | trail |
| trailcode | 428 ms, 2575896 tok | 1217 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 302 ms, 1587235 tok | 1162 ms, 628 tok | 8 ms, 3827 tok | graphify | trail |
| docs | 278 ms, 1273168 tok | 955 ms, 18 tok | unsupported | dctx | trail |

## Callers accuracy vs tool-independent regex gold (depth 1, ~20 targets per corpus; micro precision / recall)

| corpus | dctx P / R | Trail P / R | graphify P / R |
|---|---|---|---|
| flask | n/a / 0.000 | 0.962 / 0.328 | 0.933 / 0.195 |
| ripgrep | n/a / 0.000 | 0.987 / 0.327 | 1.000 / 0.032 |
| pocketbase | n/a / 0.000 | 1.000 / 0.099 | 1.000 / 0.383 |
| trailcode | n/a / 0.000 | 0.989 / 0.902 | 1.000 / 0.215 |
| ALL_GOLD_CORPORA | n/a / 0.000 | 0.986 / 0.436 | 0.989 / 0.196 |

dctx following all result pages (<=5): P n/a / R 0.000.

## Determinism / cache-friendliness

| corpus | tool | identical across 5 runs (queries) | identical after unrelated edit | ... ignoring dctx snapshot header line | changed after edit | first query after edit (s) | graphify `update` (s) |
|---|---|---|---|---|---|---|---|
| flask | dctx | 5/5 | 0/5 | 4/5 | callers1, callers2, find, overview, skeleton | 0.125 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.456 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.617 | 5.38 |
| ripgrep | dctx | 5/5 | 0/5 | 4/5 | callers1, callers2, find, overview, skeleton | 0.450 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.795 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.736 | 8.00 |
| pocketbase | dctx | 5/5 | 0/5 | 4/5 | callers1, callers2, find, overview, skeleton | 0.770 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.304 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.416 | 56.75 |
| trailcode | dctx | 5/5 | 0/5 | 4/5 | callers1, callers2, find, overview, skeleton | 1.019 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 4.248 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.481 | 46.23 |
| mixed | dctx | 3/3 | 0/3 | 2/3 | find, overview, skeleton | 0.154 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.239 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.582 | 4.62 |
| docs | dctx | 3/3 | 0/3 | 2/3 | find, overview, skeleton | 0.161 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 0.989 | n/a |
| docs | graphify | 0/0 | 0/0 | 0/0 | - | n/a | n/a |

## Privacy canary (fixtures with .env, .env.local, settings.env, config/prod.env holding CANARY strings)

| fixture | tool | canary in any tool output | canary in index artifacts | queries that leaked |
|---|---|---|---|---|
| canary-tracked | dctx | True | False | pack |
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
| scala | 2 | n/n (0/0) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | n/n (0/0) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | n/n (0/0) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | n/n (0/0) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | n/n (0/0) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | n/n (0/0) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | n/n (0/0) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | n/n (0/0) | n/n (0/0) | n/n (0/0) |
| sql | 62 | n/n (0/0) | n/n (0/0) | n/n (0/0) |
| shell | 287 | n/n (0/0) | n/n (0/0) | Y/Y (42/23) |
