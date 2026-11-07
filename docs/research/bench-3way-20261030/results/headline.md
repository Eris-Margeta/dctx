<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# 3-way benchmark headline (2026-10-30T05:04:27Z)

dctx `dctx-cli v0.25.0` sha256 `82bd48760932`; Trail-Code 0.28.0; graphify graphify 0.9.97 (code-only, no LLM). CPUs 24-31, nice 10, unshare -Urn no-network: True. Host load at start: 3.19 (shared host).

## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)

| corpus | dctx | Trail | graphify | fastest |
|---|---|---|---|---|
| flask | 0.16s, 19/7 MB, 1.1 MB | 4.17s, 225/220 MB, 4.4 MB | 8.13s, 102/134 MB, 4.5 MB | dctx |
| ripgrep | 0.54s, 25/20 MB, 2.2 MB | 3.01s, 303/302 MB, 8.4 MB | 10.45s, 93/196 MB, 8.6 MB | dctx |
| pocketbase | 0.91s, 63/53 MB, 9.9 MB | 50.63s, 987/966 MB, 41.8 MB | 58.96s, 243/476 MB, 26.3 MB | dctx |
| trailcode | 1.16s, 90/78 MB, 14.8 MB | 58.34s, 1020/1012 MB, 41.1 MB | 44.32s, 394/385 MB, 11.9 MB | dctx |
| mixed | 0.24s, 74/11 MB, 2.8 MB | 3.84s, 470/431 MB, 12.8 MB | 4.46s, 74/125 MB, 2.2 MB | dctx |
| docs | 0.18s, 74/5 MB, 1.4 MB | 1.17s, 121/119 MB, 0.0 MB | FAILED / no index | dctx |

Latency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).

### Symbol search: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 81 ms, 218 tok | 1066 ms, 262 tok | 564 ms, 1845 tok | dctx | dctx |
| ripgrep | 60 ms, 38 tok | 1267 ms, 392 tok | 791 ms, 3880 tok | dctx | dctx |
| pocketbase | 213 ms, 259 tok | 1573 ms, 358 tok | 1393 ms, 97 tok (rc=3) | dctx | graphify |
| trailcode | 197 ms, 404 tok | 1906 ms, 624 tok | 1207 ms, 1865 tok | dctx | dctx |
| mixed | 112 ms, 199 tok | 1293 ms, 282 tok | 474 ms, 206 tok | dctx | dctx |
| docs | 102 ms, 359 tok | 1043 ms, 92 tok | unsupported | dctx | trail |

### Callers depth 1: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 47 ms, 479 tok | 988 ms, 136 tok | 381 ms, 32 tok | dctx | graphify |
| ripgrep | 75 ms, 484 tok | 1111 ms, 801 tok | 450 ms, 29 tok | dctx | graphify |
| pocketbase | 214 ms, 464 tok | 1195 ms, 175 tok | 777 ms, 24 tok | dctx | graphify |
| trailcode | 200 ms, 472 tok | 1180 ms, 1064 tok | 715 ms, 196 tok | dctx | graphify |
| mixed | unsupported | unsupported | unsupported | - | - |
| docs | unsupported | unsupported | unsupported | - | - |

### File skeleton / outline: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 44 ms, 866 tok | 991 ms, 852 tok | 514 ms, 474 tok | dctx | graphify |
| ripgrep | 68 ms, 1784 tok | 1080 ms, 1125 tok | 714 ms, 533 tok | dctx | graphify |
| pocketbase | 212 ms, 383 tok | 1162 ms, 351 tok | 1288 ms, 813 tok | dctx | trail |
| trailcode | 183 ms, 784 tok | 1198 ms, 770 tok | 1086 ms, 759 tok | dctx | graphify |
| mixed | 110 ms, 1162 tok | 1031 ms, 1394 tok | 409 ms, 414 tok | dctx | graphify |
| docs | 99 ms, 933 tok | 1080 ms, 29 tok | unsupported | dctx | trail |

### Repo overview: median ms | est tokens

| corpus | dctx | Trail | graphify | fastest | smallest answer |
|---|---|---|---|---|---|
| flask | 45 ms, 491 tok | 1009 ms, 1122 tok | 7 ms, 5930 tok | graphify | dctx |
| ripgrep | 63 ms, 590 tok | 1145 ms, 2538 tok | 8 ms, 7960 tok | graphify | dctx |
| pocketbase | 226 ms, 652 tok | 1232 ms, 1514 tok | 8 ms, 11591 tok | graphify | dctx |
| trailcode | 195 ms, 500 tok | 1159 ms, 499 tok | 7 ms, 10076 tok | graphify | trail |
| mixed | 109 ms, 84 tok | 1148 ms, 628 tok | 7 ms, 3827 tok | graphify | dctx |
| docs | 99 ms, 30 tok | 978 ms, 18 tok | unsupported | dctx | trail |

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
| flask | dctx | 6/6 | 5/6 | 5/6 | overview | 0.132 | n/a |
| flask | trail | 7/7 | 6/7 | 6/7 | overview | 1.493 | n/a |
| flask | graphify | 6/6 | 3/6 | 3/6 | find, overview, skeleton | 0.629 | 5.90 |
| ripgrep | dctx | 6/6 | 5/6 | 5/6 | overview | 0.533 | n/a |
| ripgrep | trail | 7/7 | 6/7 | 6/7 | overview | 1.995 | n/a |
| ripgrep | graphify | 5/6 | 3/6 | 3/6 | find, overview, skeleton | 0.805 | 8.08 |
| pocketbase | dctx | 6/6 | 5/6 | 5/6 | overview | 0.962 | n/a |
| pocketbase | trail | 7/7 | 6/7 | 6/7 | overview | 3.968 | n/a |
| pocketbase | graphify | 6/6 | 3/6 | 3/6 | god_nodes, overview, skeleton | 1.527 | 56.22 |
| trailcode | dctx | 6/6 | 5/6 | 5/6 | overview | 1.282 | n/a |
| trailcode | trail | 7/7 | 6/7 | 6/7 | overview | 4.039 | n/a |
| trailcode | graphify | 6/6 | 2/6 | 2/6 | find, god_nodes, overview, skeleton | 1.334 | 37.08 |
| mixed | dctx | 4/4 | 3/4 | 3/4 | overview | 0.246 | n/a |
| mixed | trail | 4/4 | 4/4 | 4/4 | - | 1.379 | n/a |
| mixed | graphify | 4/4 | 0/4 | 0/4 | find, god_nodes, overview, skeleton | 0.561 | 4.70 |
| docs | dctx | 4/4 | 3/4 | 3/4 | overview | 0.196 | n/a |
| docs | trail | 4/4 | 4/4 | 4/4 | - | 0.980 | n/a |
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
| trail | default_env | build, callers, ask | 49 | connect 104.16.0.34:0 -> -Network is unreachable; connect 104.16.0.34:443 -> -Network is unreachable; connect 104.16.1.34:0 -> -Network is unreachable | /run/systemd/resolve/io.systemd.Resolve, /var/run/nscd/socket |
| graphify | default_env | extract, query | 0 | - | - |

## Language coverage on the mixed corpus (>=1 symbol / >=1 call edge from the 6 largest files per language)

| language | files | dctx sym/edge | Trail sym/edge | graphify sym/edge |
|---|---|---|---|---|
| scala | 2 | Y/n (5/0) | Y/Y (312/1) | Y/Y (76/32) |
| dart | 25 | Y/n (16/0) | Y/Y (83/10) | n/n (0/0) |
| lua | 3 | Y/n (10/0) | Y/Y (106/1) | Y/Y (76/34) |
| elixir | 9 | Y/n (17/0) | Y/Y (142/4) | Y/Y (45/44) |
| zig | 4 | Y/n (11/0) | Y/Y (284/5) | Y/Y (27/29) |
| ocaml | 51 | Y/n (25/0) | Y/Y (2559/7) | n/n (0/0) |
| solidity | 59 | Y/n (24/0) | Y/Y (419/2) | n/n (0/0) |
| terraform | 77 | Y/n (22/0) | n/n (0/0) | n/n (0/0) |
| sql | 62 | Y/n (21/0) | n/n (0/0) | n/n (0/0) |
| shell | 287 | Y/n (8/0) | n/n (0/0) | Y/Y (42/23) |
