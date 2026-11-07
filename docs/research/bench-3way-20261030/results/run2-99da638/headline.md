<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# 3-way benchmark headline (2026-10-30T07:24:19Z)

dctx `dctx-cli v0.25.0` sha256 `81b35b3eb511`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 3.45 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.13s, 20/6 MB, 1.1 MB | 4.15s, 226/224 MB, 4.4 MB | 7.40s, 103/135 MB, 4.5 MB | dctx |
| ripgrep | 0.48s, 26/21 MB, 2.2 MB | 2.84s, 304/302 MB, 8.4 MB | 10.30s, 93/197 MB, 8.6 MB | dctx |
| pocketbase | 0.91s, 63/48 MB, 9.9 MB | 51.35s, 991/966 MB, 41.8 MB | 65.87s, 243/482 MB, 26.3 MB | dctx |
| trailcode | 1.06s, 90/77 MB, 14.8 MB | 53.01s, 1013/1004 MB, 41.1 MB | 45.93s, 393/385 MB, 11.9 MB | dctx |
| mixed | 0.28s, 74/14 MB, 3.3 MB | 3.54s, 460/454 MB, 12.8 MB | 4.40s, 74/126 MB, 2.2 MB | dctx |
| docs | 0.18s, 74/5 MB, 1.4 MB | 1.12s, 122/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 218 tok | 1015 ms, 262 tok | 541 ms, 1845 tok | dctx | dctx |
| ripgrep | 59 ms, 38 tok | 1162 ms, 392 tok | 716 ms, 3880 tok | dctx | dctx |
| pocketbase | 210 ms, 259 tok | 1456 ms, 358 tok | 1380 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 184 ms, 404 tok | 1782 ms, 624 tok | 1220 ms, 1865 tok | dctx | dctx |
| mixed | 110 ms, 199 tok | 1214 ms, 282 tok | 469 ms, 206 tok | dctx | dctx |
| docs | 94 ms, 359 tok | 1033 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 479 tok | 984 ms, 136 tok | 344 ms, 32 tok | dctx | graphify |
| ripgrep | 59 ms, 484 tok | 964 ms, 801 tok | 411 ms, 29 tok | dctx | graphify |
| pocketbase | 191 ms, 464 tok | 1103 ms, 175 tok | 795 ms, 24 tok | dctx | graphify |
| trailcode | 183 ms, 472 tok | 1109 ms, 1064 tok | 685 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 43 ms, 866 tok | 962 ms, 852 tok | 484 ms, 474 tok | dctx | graphify |
| ripgrep | 60 ms, 1784 tok | 972 ms, 1125 tok | 645 ms, 533 tok | dctx | graphify |
| pocketbase | 197 ms, 383 tok | 1072 ms, 351 tok | 1321 ms, 813 tok | dctx | trail |
| trailcode | 183 ms, 784 tok | 1097 ms, 770 tok | 1135 ms, 759 tok | dctx | graphify |
| mixed | 110 ms, 1162 tok | 1043 ms, 1394 tok | 409 ms, 414 tok | dctx | graphify |
| docs | 90 ms, 933 tok | 1015 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 491 tok | 1010 ms, 1122 tok | 7 ms, 5930 tok | graphify | dctx |
| ripgrep | 62 ms, 590 tok | 1033 ms, 2538 tok | 7 ms, 7960 tok | graphify | dctx |
| pocketbase | 216 ms, 652 tok | 1111 ms, 1514 tok | 7 ms, 11591 tok | graphify | dctx |
| trailcode | 202 ms, 500 tok | 1132 ms, 499 tok | 8 ms, 10076 tok | graphify | trail |
| mixed | 113 ms, 603 tok | 1035 ms, 628 tok | 7 ms, 3827 tok | graphify | dctx |
| docs | 89 ms, 30 tok | 1278 ms, 18 tok | unsupported | dctx | trail |

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
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.121 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.346 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.559 | 5.32 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.481 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.798 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.733 | 7.98 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 0.963 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.240 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.395 | 54.39 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.100 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 3.889 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.244 | 38.33 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.329 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.351 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.634 | 4.88 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.199 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 1.317 | n/a |
| docs | graphify | 0/0 | 0/0 | 0/0 | - | n/a | n/a |

## Privacy canary (fixtures with .env, .env.local, settings.env, config/prod.env holding CANARY strings)

| fixture | tool | canary in any tool output | canary in index artifacts | queries that leaked |
|---|---|---|---|---|
| canary-tracked | dctx | True | False | read_env, read_settings |
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

| language | files | dctx sym/edge | Trail sym/edge | graphify sym/edge |
|---|---|---|---|---|
| scala | 2 | Y/n (5/0) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | Y/Y (16/3) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/n (10/0) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/n (17/0) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/Y (11/3) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/n (25/0) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/Y (24/3) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/Y (22/4) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/Y (21/2) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/Y (8/2) | n/n (0/0) | Y/Y (42/23) |
