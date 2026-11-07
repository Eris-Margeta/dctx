<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
| Corpus | dctx calls / tokens / score / facts | trail calls / tokens / score / facts | graphify calls / tokens / score / facts | dctx-ext calls / tokens / score / facts |
|---|---|---|---|---|
| flask | 15 / 3,604 / 20 of 20 / 47 of 48 | 10 / 5,511 / 16 of 20 / 39 of 48 | 13 / 10,447 / 14 of 20 / 34 of 48 | 15 / 5,338 / 20 of 20 / 47 of 48 |
| ripgrep | 15 / 3,563 / 19 of 20 / 30 of 32 | 10 / 25,974 / 13 of 20 / 23 of 32 | 13 / 18,709 / 12 of 20 / 22 of 32 | 15 / 5,407 / 17 of 20 / 24 of 32 |
| pocketbase | 15 / 3,533 / 19 of 20 / 35 of 36 | 10 / 4,026 / 18 of 20 / 33 of 36 | 13 / 18,240 / 13 of 20 / 21 of 36 | 15 / 4,523 / 19 of 20 / 35 of 36 |
| trailcode | 15 / 3,578 / 20 of 20 / 64 of 64 | 10 / 7,779 / 19 of 20 / 59 of 64 | 13 / 17,789 / 15 of 20 / 49 of 64 | 15 / 5,359 / 19 of 20 / 62 of 64 |
| dctx | 15 / 4,298 / 20 of 20 / 47 of 48 | 10 / 15,396 / 19 of 20 / 44 of 48 | 13 / 29,479 / 14 of 20 / 33 of 48 | 15 / 5,862 / 18 of 20 / 41 of 48 |
| django | 15 / 4,987 / 19 of 20 / 67 of 70 | 10 / 7,006 / 18 of 20 / 63 of 70 | 13 / 50,123 / 12 of 20 / 34 of 70 | 15 / 6,983 / 18 of 20 / 64 of 70 |
| core | 15 / 4,909 / 19 of 20 / 52 of 56 | 10 / 23,231 / 19 of 20 / 51 of 56 | 13 / 52,416 / 16 of 20 / 45 of 56 | 15 / 7,444 / 19 of 20 / 52 of 56 |
| **All** | 105 / 28,472 / **136 of 140** / 342 of 354 (12.01 facts/1k tok) | 70 / 88,923 / **122 of 140** / 312 of 354 (3.51 facts/1k tok) | 91 / 197,203 / **96 of 140** / 238 of 354 (1.21 facts/1k tok) | 105 / 40,916 / **130 of 140** / 325 of 354 (7.94 facts/1k tok) |
| All except overview | 98 / 24,264 / **122 of 126** / 310 of 322 (12.78 facts/1k tok) | 63 / 57,221 / **109 of 126** / 284 of 322 (4.96 facts/1k tok) | 84 / 50,648 / **85 of 126** / 218 of 322 (4.3 facts/1k tok) | 98 / 36,708 / **116 of 126** / 293 of 322 (7.98 facts/1k tok) |

By question type (score, out of 2 x number of corpora run):

| Type | dctx | trail | graphify | dctx-ext |
|---|---|---|---|---|
| def_and_callers | 14/14, 40/40 facts, 4,550 tok | 10/14, 29/40 facts, 3,379 tok | 10/14, 23/40 facts, 5,127 tok | 12/14, 34/40 facts, 4,671 tok |
| impact | 14/14, 17/17 facts, 1,685 tok | 13/14, 16/17 facts, 1,730 tok | 12/14, 15/17 facts, 1,856 tok | 14/14, 17/17 facts, 2,844 tok |
| outline | 14/14, 115/115 facts, 2,943 tok | 14/14, 114/115 facts, 5,676 tok | 12/14, 85/115 facts, 7,830 tok | 14/14, 115/115 facts, 6,089 tok |
| config | 14/14, 20/20 facts, 907 tok | 14/14, 19/20 facts, 5,512 tok | 6/14, 6/20 facts, 7,914 tok | 12/14, 15/20 facts, 2,582 tok |
| exports | 14/14, 50/50 facts, 3,613 tok | 14/14, 50/50 facts, 24,902 tok | 14/14, 49/50 facts, 11,582 tok | 12/14, 44/50 facts, 7,391 tok |
| source | 14/14, 20/20 facts, 2,831 tok | 12/14, 17/20 facts, 7,539 tok | 0/14, 0/20 facts, 865 tok | 14/14, 20/20 facts, 2,831 tok |
| callees | 10/14, 26/38 facts, 2,568 tok | 8/14, 20/38 facts, 1,238 tok | 8/14, 21/38 facts, 1,658 tok | 10/14, 26/38 facts, 4,242 tok |
| overview | 14/14, 32/32 facts, 4,208 tok | 13/14, 28/32 facts, 31,702 tok | 11/14, 20/32 facts, 146,555 tok | 14/14, 32/32 facts, 4,208 tok |
| collision | 14/14, 15/15 facts, 3,395 tok | 12/14, 13/15 facts, 3,613 tok | 13/14, 14/15 facts, 1,371 tok | 14/14, 15/15 facts, 4,286 tok |
| concept | 14/14, 7/7 facts, 1,772 tok | 12/14, 6/7 facts, 3,632 tok | 10/14, 5/7 facts, 12,445 tok | 14/14, 7/7 facts, 1,772 tok |
