<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
| Corpus | dctx calls / tokens / score / facts | trail calls / tokens / score / facts | graphify calls / tokens / score / facts | dctx-ext calls / tokens / score / facts |
|---|---|---|---|---|
| flask | 15 / 3,873 / 13 of 20 / 34 of 48 | 10 / 5,511 / 16 of 20 / 39 of 48 | 13 / 10,447 / 14 of 20 / 34 of 48 | 15 / 5,483 / 15 of 20 / 38 of 48 |
| ripgrep | 15 / 2,959 / 10 of 20 / 18 of 32 | 10 / 25,974 / 13 of 20 / 23 of 32 | 13 / 18,709 / 12 of 20 / 22 of 32 | 15 / 5,122 / 12 of 20 / 19 of 32 |
| pocketbase | 15 / 3,380 / 12 of 20 / 28 of 36 | 10 / 4,026 / 18 of 20 / 33 of 36 | 13 / 18,240 / 13 of 20 / 21 of 36 | 15 / 4,339 / 15 of 20 / 30 of 36 |
| trailcode | 15 / 3,495 / 12 of 20 / 41 of 64 | 10 / 7,779 / 19 of 20 / 59 of 64 | 13 / 17,789 / 15 of 20 / 49 of 64 | 15 / 5,153 / 15 of 20 / 53 of 64 |
| dctx | 15 / 4,290 / 12 of 20 / 26 of 48 | 10 / 15,396 / 19 of 20 / 44 of 48 | 13 / 29,479 / 14 of 20 / 33 of 48 | 15 / 5,551 / 13 of 20 / 32 of 48 |
| django | 15 / 4,007 / 10 of 20 / 35 of 70 | 10 / 7,006 / 18 of 20 / 63 of 70 | 13 / 50,123 / 12 of 20 / 34 of 70 | 15 / 6,580 / 13 of 20 / 54 of 70 |
| core | 15 / 3,925 / 8 of 20 / 13 of 56 | 10 / 23,231 / 19 of 20 / 51 of 56 | 13 / 52,416 / 16 of 20 / 45 of 56 | 15 / 7,087 / 13 of 20 / 47 of 56 |
| **All** | 105 / 25,929 / **77 of 140** / 195 of 354 (7.52 facts/1k tok) | 70 / 88,923 / **122 of 140** / 312 of 354 (3.51 facts/1k tok) | 91 / 197,203 / **96 of 140** / 238 of 354 (1.21 facts/1k tok) | 105 / 39,315 / **96 of 140** / 273 of 354 (6.94 facts/1k tok) |
| All except overview | 98 / 21,709 / **63 of 126** / 163 of 322 (7.51 facts/1k tok) | 63 / 57,221 / **109 of 126** / 284 of 322 (4.96 facts/1k tok) | 84 / 50,648 / **85 of 126** / 218 of 322 (4.3 facts/1k tok) | 98 / 35,095 / **82 of 126** / 241 of 322 (6.87 facts/1k tok) |

By question type (score, out of 2 x number of corpora run):

| Type | dctx | trail | graphify | dctx-ext |
|---|---|---|---|---|
| def_and_callers | 7/14, 7/40 facts, 4,454 tok | 10/14, 29/40 facts, 3,379 tok | 10/14, 23/40 facts, 5,127 tok | 7/14, 9/40 facts, 4,526 tok |
| impact | 7/14, 7/17 facts, 2,095 tok | 13/14, 16/17 facts, 1,730 tok | 12/14, 15/17 facts, 1,856 tok | 7/14, 7/17 facts, 3,202 tok |
| outline | 11/14, 73/115 facts, 2,668 tok | 14/14, 114/115 facts, 5,676 tok | 12/14, 85/115 facts, 7,830 tok | 14/14, 115/115 facts, 6,089 tok |
| config | 0/14, 0/20 facts, 839 tok | 14/14, 19/20 facts, 5,512 tok | 6/14, 6/20 facts, 7,914 tok | 12/14, 15/20 facts, 2,582 tok |
| exports | 8/14, 24/50 facts, 2,937 tok | 14/14, 50/50 facts, 24,902 tok | 14/14, 49/50 facts, 11,582 tok | 12/14, 44/50 facts, 7,391 tok |
| source | 14/14, 20/20 facts, 2,831 tok | 12/14, 17/20 facts, 7,539 tok | 0/14, 0/20 facts, 865 tok | 14/14, 20/20 facts, 2,831 tok |
| callees | 10/14, 26/38 facts, 2,568 tok | 8/14, 20/38 facts, 1,238 tok | 8/14, 21/38 facts, 1,658 tok | 9/14, 24/38 facts, 4,283 tok |
| overview | 14/14, 32/32 facts, 4,220 tok | 13/14, 28/32 facts, 31,702 tok | 11/14, 20/32 facts, 146,555 tok | 14/14, 32/32 facts, 4,220 tok |
| collision | 6/14, 6/15 facts, 3,285 tok | 12/14, 13/15 facts, 3,613 tok | 13/14, 14/15 facts, 1,371 tok | 7/14, 7/15 facts, 4,159 tok |
| concept | 0/14, 0/7 facts, 32 tok | 12/14, 6/7 facts, 3,632 tok | 10/14, 5/7 facts, 12,445 tok | 0/14, 0/7 facts, 32 tok |
