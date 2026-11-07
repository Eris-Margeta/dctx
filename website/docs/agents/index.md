# AGENTS.md snippet: task context from dctx

Paste the section below into a project's `AGENTS.md` (or `CLAUDE.md`) so coding agents start from dctx context.
This repository keeps its own agent files out of git, so the snippet lives here.

## Notes for coding agents

Before editing, pull the code your task names, cut to a budget:

```sh
dctx pack --for "$TASK" --budget 3000
```

`$TASK` is the task or card text. dctx runs `map find` on the names the text mentions (backticked words, snake_case,
camelCase, `a::b`, then longer plain words), then `map neighbors` and `map read` on the best hits, and prints whole lines
until the budget is spent. The budget is in estimated tokens, ceil(2 × UTF-8 bytes / 7), not a tokenizer count. The last
line reports the estimate and how many lines were dropped. Exit 1 means no name in the text matched the index.

For a card file, `tools/cardgen/context.sh CARD [PLACE] [BUDGET]` prints the card followed by this context.
Then narrow with `dctx map find`, `dctx map neighbors <id>` and `dctx map read <id>` as needed (`dctx map --help`).
