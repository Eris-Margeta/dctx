<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
| Corpus | dctx calls / tokens / score / facts | trail calls / tokens / score / facts | graphify calls / tokens / score / facts | dctx-ext calls / tokens / score / facts |
|---|---|---|---|---|
| flask | 15 / 4,211 / 16 of 20 / 42 of 48 | 10 / 5,511 / 16 of 20 / 39 of 48 | 13 / 10,447 / 14 of 20 / 34 of 48 | 15 / 5,586 / 17 of 20 / 42 of 48 |
| ripgrep | 15 / 3,420 / 14 of 20 / 22 of 32 | 10 / 25,974 / 13 of 20 / 23 of 32 | 13 / 18,709 / 12 of 20 / 22 of 32 | 15 / 5,420 / 14 of 20 / 22 of 32 |
| pocketbase | 15 / 3,729 / 15 of 20 / 32 of 36 | 10 / 4,026 / 18 of 20 / 33 of 36 | 13 / 18,240 / 13 of 20 / 21 of 36 | 15 / 4,675 / 16 of 20 / 33 of 36 |
| trailcode | 15 / 3,805 / 15 of 20 / 50 of 64 | 10 / 7,779 / 19 of 20 / 59 of 64 | 13 / 17,789 / 15 of 20 / 49 of 64 | 15 / 5,345 / 17 of 20 / 59 of 64 |
| dctx | 15 / 4,711 / 17 of 20 / 37 of 48 | 10 / 15,396 / 19 of 20 / 44 of 48 | 13 / 29,479 / 14 of 20 / 33 of 48 | 15 / 5,986 / 17 of 20 / 38 of 48 |
| django | 15 / 4,455 / 13 of 20 / 43 of 70 | 10 / 7,006 / 18 of 20 / 63 of 70 | 13 / 50,123 / 12 of 20 / 34 of 70 | 15 / 7,042 / 15 of 20 / 60 of 70 |
| core | 15 / 4,411 / 12 of 20 / 17 of 56 | 10 / 23,231 / 19 of 20 / 51 of 56 | 13 / 52,416 / 16 of 20 / 45 of 56 | 15 / 7,475 / 15 of 20 / 49 of 56 |
| **All** | 105 / 28,742 / **102 of 140** / 243 of 354 (8.45 facts/1k tok) | 70 / 88,923 / **122 of 140** / 312 of 354 (3.51 facts/1k tok) | 91 / 197,203 / **96 of 140** / 238 of 354 (1.21 facts/1k tok) | 105 / 41,529 / **111 of 140** / 303 of 354 (7.3 facts/1k tok) |
| All except overview | 98 / 24,522 / **88 of 126** / 211 of 322 (8.6 facts/1k tok) | 63 / 57,221 / **109 of 126** / 284 of 322 (4.96 facts/1k tok) | 84 / 50,648 / **85 of 126** / 218 of 322 (4.3 facts/1k tok) | 98 / 37,309 / **97 of 126** / 271 of 322 (7.26 facts/1k tok) |

By question type (score, out of 2 x number of corpora run):

| Type | dctx | trail | graphify | dctx-ext |
|---|---|---|---|---|
| def_and_callers | 12/14, 34/40 facts, 4,543 tok | 10/14, 29/40 facts, 3,379 tok | 10/14, 23/40 facts, 5,127 tok | 9/14, 23/40 facts, 4,482 tok |
| impact | 14/14, 17/17 facts, 2,114 tok | 13/14, 16/17 facts, 1,730 tok | 12/14, 15/17 facts, 1,856 tok | 14/14, 17/17 facts, 3,182 tok |
| outline | 11/14, 73/115 facts, 2,668 tok | 14/14, 114/115 facts, 5,676 tok | 12/14, 85/115 facts, 7,830 tok | 14/14, 115/115 facts, 6,089 tok |
| config | 7/14, 5/20 facts, 1,294 tok | 14/14, 19/20 facts, 5,512 tok | 6/14, 6/20 facts, 7,914 tok | 12/14, 15/20 facts, 2,582 tok |
| exports | 8/14, 24/50 facts, 2,937 tok | 14/14, 50/50 facts, 24,902 tok | 14/14, 49/50 facts, 11,582 tok | 12/14, 44/50 facts, 7,391 tok |
| source | 14/14, 20/20 facts, 2,831 tok | 12/14, 17/20 facts, 7,539 tok | 0/14, 0/20 facts, 865 tok | 14/14, 20/20 facts, 2,831 tok |
| callees | 10/14, 26/38 facts, 2,568 tok | 8/14, 20/38 facts, 1,238 tok | 8/14, 21/38 facts, 1,658 tok | 9/14, 24/38 facts, 4,283 tok |
| overview | 14/14, 32/32 facts, 4,220 tok | 13/14, 28/32 facts, 31,702 tok | 11/14, 20/32 facts, 146,555 tok | 14/14, 32/32 facts, 4,220 tok |
| collision | 10/14, 11/15 facts, 3,305 tok | 12/14, 13/15 facts, 3,613 tok | 13/14, 14/15 facts, 1,371 tok | 11/14, 12/15 facts, 4,207 tok |
| concept | 2/14, 1/7 facts, 2,262 tok | 12/14, 6/7 facts, 3,632 tok | 10/14, 5/7 facts, 12,445 tok | 2/14, 1/7 facts, 2,262 tok |
