#!/usr/bin/env python3
"""Dogfood run of `dctx pack --for` on the ten v1 finish cards (v1 card 9).

For each card in docs/research/dogfood/cards/*.md, runs tools/cardgen/context.sh on this repository and records the
prompt size before (the card alone) and after (the card plus its dctx context), in one tokenizer for every number:
the dctx estimate ceil(2 * UTF-8 bytes / 7) by default, or `--tokenizer cl100k` (tiktoken cl100k_base, which must
already be installed and cached: this script makes no network call). The tokenizer used is recorded. The context
part must stay within the budget by the dctx estimate and hold at least one find section.

Usage: python3 tools/cardgen/dogfood.py --dctx PATH/TO/dctx-cli [--budget 3000] [--tokenizer estimate|cl100k] [--out FILE]
Prints `DOGFOOD PASS cards=<n> ...` and exits 0, or `DOGFOOD FAIL: <why>` and exits 1.
Default output: docs/research/dogfood/results/dogfood.json (no timestamps, so a rerun on the same tree is identical).
"""

import argparse
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
CARDS = ROOT / "docs" / "research" / "dogfood" / "cards"
CONTEXT = ROOT / "tools" / "cardgen" / "context.sh"
DEFAULT_OUT = ROOT / "docs" / "research" / "dogfood" / "results" / "dogfood.json"
MARKER = "\n\n## dctx context (dctx pack --for, budget "


def estimate(text):
    """The dctx estimate: ceil(2 * UTF-8 bytes / 7)."""
    return -(-2 * len(text.encode("utf-8")) // 7)


def tokenizer(choice):
    """Returns (name, count function). `cl100k` needs tiktoken with its encoding already cached; it never installs one."""
    if choice == "estimate":
        return "dctx estimate ceil(2*bytes/7)", estimate
    import tiktoken  # noqa: PLC0415 - optional dependency, only for --tokenizer cl100k

    encoding = tiktoken.get_encoding("cl100k_base")
    return "tiktoken cl100k_base", lambda text: len(encoding.encode(text, disallowed_special=()))


def run_card(card, dctx, budget):
    # HOME, DCTX_CACHE_DIR and TMPDIR pass through: the map cache lives where the caller's dctx keeps it.
    env = dict(os.environ, DCTX=dctx, DCTX_AUTO_CLEAN="0", NO_COLOR="1")
    done = subprocess.run(
        ["sh", str(CONTEXT), str(card), str(ROOT), str(budget)],
        cwd=ROOT, env=env, capture_output=True, text=True, check=False,
    )
    return done.returncode, done.stdout, done.stderr


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dctx", required=True, help="dctx-cli binary to run")
    parser.add_argument("--budget", type=int, default=3000)
    parser.add_argument("--out", default=str(DEFAULT_OUT))
    parser.add_argument("--tokenizer", choices=["estimate", "cl100k"], default="estimate")
    args = parser.parse_args()
    dctx = str(pathlib.Path(args.dctx).resolve())
    name, count = tokenizer(args.tokenizer)
    cards = sorted(CARDS.glob("card-*.md"))
    if len(cards) < 10:
        print(f"DOGFOOD FAIL: {len(cards)} cards in {CARDS}, need 10")
        return 1
    rows, failures = [], []
    for card in cards:
        rc, out, err = run_card(card, dctx, args.budget)
        text = card.read_text(encoding="utf-8")
        context = out.split(MARKER, 1)[1].split("\n", 2)[-1] if MARKER in out else ""
        row = {
            "card": card.name,
            "exit": rc,
            "before_tokens": count(text),
            "after_tokens": count(out),
            "context_tokens": count(context),
            "context_estimate": estimate(context),
            "find_sections": context.count("\n## find "),
            "neighbors_sections": context.count("## neighbors "),
            "read_sections": context.count("## read "),
        }
        rows.append(row)
        if rc != 0:
            failures.append(f"{card.name}: exit {rc}: {err.strip()[:200]}")
        elif row["context_estimate"] > args.budget:
            failures.append(f"{card.name}: context {row['context_estimate']} > budget {args.budget}")
        elif row["find_sections"] == 0:
            failures.append(f"{card.name}: no find section in the context")
    version = subprocess.run([dctx, "--version"], capture_output=True, text=True, check=False).stdout.strip()
    before = sum(row["before_tokens"] for row in rows)
    after = sum(row["after_tokens"] for row in rows)
    report = {
        "dctx": version,
        "tokenizer": name,
        "budget": args.budget,
        "estimate": "ceil(2 * UTF-8 bytes / 7)",
        "cards": rows,
        "totals": {"cards": len(rows), "before_tokens": before, "after_tokens": after, "delta_tokens": after - before},
    }
    out_path = pathlib.Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if failures:
        print("DOGFOOD FAIL: " + "; ".join(failures))
        return 1
    print(f"DOGFOOD PASS cards={len(rows)} before={before} after={after} delta={after - before} tokenizer={name} out={out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
