# Card 10: robustness corpus

Goal: hostile inputs. Files: `bench/corpora/hostile/` (empty repo, binary, non-UTF8, CRLF, 50 MB generated file, symlink loop, nested `.gitignore`, `.git` worktree), `tests/hostile.rs`. Gate: all commands exit 0/1, never 101, within time budget.
