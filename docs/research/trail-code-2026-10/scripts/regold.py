#!/usr/bin/env python3
"""Recompute gold call sites for an existing target list with the current oracle (targets unchanged).

usage: regold.py PRISTINE_ROOT TARGET_DIR CORPUS [CORPUS...]
Used once after the oracle's comment/string blanking was made multi-line aware (see REPORT.md,
methodology): the selected definitions stay fixed, only their adjudicated call sites are rebuilt.
"""
import json
import os
import sys

from goldlib import Corpus
from select_targets import gold_for


def main():
    root, tdir = sys.argv[1], sys.argv[2]
    for corpus in sys.argv[3:]:
        p = os.path.join(tdir, f"{corpus}.targets.json")
        with open(p) as f:
            doc = json.load(f)
        c = Corpus(os.path.join(root, corpus))
        changed = 0
        for t in doc["targets"]:
            d0 = t["def"]
            d = next((x for x in c.defs if x.file == d0["file"] and x.line == d0["line"] and x.name == d0["name"]), None)
            if d is None:
                print("missing def", corpus, d0["name"], d0["file"], d0["line"])
                continue
            old = {k: len(v) for k, v in t["gold"].items()}
            t["gold"] = gold_for(c, d)
            t["def"] = d.as_dict()
            t["n_defs_same_name"] = len(c.callable_defs(d.name))
            new = {k: len(v) for k, v in t["gold"].items()}
            if old != new:
                changed += 1
                print(f"  {corpus} {d.name}: {old} -> {new}")
        doc["oracle_note"] = "gold recomputed by regold.py with the multi-line-aware cleaner"
        with open(p, "w") as f:
            json.dump(doc, f, indent=1)
        print(corpus, "targets with changed gold:", changed, flush=True)


if __name__ == "__main__":
    main()
