<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# AGENTS.md snippet: task context from dctx

Paste the section below into a project's `AGENTS.md` (or `CLAUDE.md`) so coding agents start from dctx context.

## Notes for coding agents

Before editing, pull the code your task names, cut to a budget:

```sh
dctx pack --for "$TASK" --budget 3000
```

`$TASK` is the task description. dctx runs `map find` on the names the text mentions (backticked words, snake_case,
camelCase, `a::b`, then longer plain words), then `map neighbors` and `map read` on the best hits, and prints whole lines
until the budget is spent. The budget is in estimated tokens, ceil(2 × UTF-8 bytes / 7), not a tokenizer count. The last
line reports the estimate and how many lines were dropped. Exit 1 means no name in the text matched the index.

Name things in the text: `--for main` or `--for Session` works, but a sentence such as "how does caching work" only
matches loose words, and dctx says so on stderr (the page itself is unchanged). Put a short name next to prose in
backticks, as in ``--for 'fix `run` in the pack command'``.

Then narrow with `dctx map find`, `dctx map neighbors <id>` and `dctx map read <id>` as needed (`dctx map --help`).
