#!/usr/bin/env python3
"""Select ~20 accuracy targets per corpus (seeded, reproducible) and write gold call sites.

usage: select_targets.py PRISTINE_ROOT OUT_DIR CORPUS [CORPUS...]

Categories (4 each): hub, rare, receiver-method, collision, interface/trait.
Writes OUT_DIR/<corpus>.targets.json.
"""
import json
import os
import random
import re
import sys
import zlib

from goldlib import Corpus, KEYWORDS, STD_METHODS

TESTY = re.compile(r"(^|/)(tests?|testdata|testing|fixtures?|benches|examples?|docs?)(/|$)|_test\.|(^|/)test_|\.test\.|\.spec\.|conftest")


def eligible(d):
    n = d.name
    return (len(n) >= 5 and n not in KEYWORDS and n not in STD_METHODS and not n.startswith("__") and not n.lower().startswith("test")
            and not TESTY.search(d.file) and d.kind in ("function", "method"))


def gold_for(c, d):
    sites = c.call_sites(d.name)
    out = {"assigned": [], "other_def": [], "ambiguous": []}
    for s in sites:
        r = c.resolve(s, d.name)
        if r is None:
            out["ambiguous"].append(s)
        elif r == "other":
            out["other_def"].append(s)
        elif r.key() == d.key():
            out["assigned"].append(s)
        else:
            s = dict(s, bound_to=r.key())
            out["other_def"].append(s)
    return out


def main():
    root, outdir = sys.argv[1], sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    for corpus in sys.argv[3:]:
        c = Corpus(os.path.join(root, corpus))
        rng = random.Random(zlib.crc32(corpus.encode()))
        defs = [d for d in c.defs if eligible(d)]
        ndefs = {n: len(c.callable_defs(n)) for n in {d.name for d in defs}}
        picked, chosen = [], set()

        def take(cat, pool, k=4):
            pool = [d for d in pool if d.name not in chosen]
            rng.shuffle(pool)
            got = 0
            for d in pool:
                if got >= k:
                    break
                g = gold_for(c, d)
                need = 1 if cat != "rare" else 1
                if len(g["assigned"]) < need and cat != "interface":
                    continue
                if cat == "interface" and len(g["assigned"]) + len(g["ambiguous"]) + len(g["other_def"]) < 2:
                    continue
                picked.append({"category": cat, "def": d.as_dict(), "n_defs_same_name": ndefs[d.name],
                               "gold": g})
                chosen.add(d.name)
                got += 1
            return got

        cc = c.call_counts
        uniq = [d for d in defs if ndefs[d.name] == 1]
        hubs = sorted([d for d in uniq if cc[d.name] >= 12], key=lambda d: -cc[d.name])[:40]
        take("hub", hubs)
        take("rare", [d for d in uniq if 1 <= cc[d.name] <= 3])
        take("receiver", [d for d in uniq if d.kind == "method" and cc[d.name] >= 2])
        take("collision", [d for d in defs if 2 <= ndefs[d.name] <= 6 and cc[d.name] >= 2 and not d.is_iface])
        n = take("interface", [d for d in defs if d.is_iface and ndefs[d.name] <= 10])
        if n < 4:
            # fall back: a method name implemented by several types (dynamic dispatch)
            take("interface", [d for d in defs if d.kind == "method" and 3 <= ndefs[d.name] <= 10 and cc[d.name] >= 2],
                 4 - n)
        while len(picked) < 20:
            if take("hub-fill", [d for d in uniq if cc[d.name] >= 4], 20 - len(picked)) == 0:
                break
        doc = {"corpus": corpus, "files_scanned": len(c.cleaned), "defs_scanned": len(c.defs), "targets": picked}
        with open(os.path.join(outdir, f"{corpus}.targets.json"), "w") as f:
            json.dump(doc, f, indent=1)
        print(corpus, len(c.cleaned), "files", len(c.defs), "defs", len(picked), "targets",
              {k: sum(1 for p in picked if p["category"] == k) for k in
               ("hub", "rare", "receiver", "collision", "interface", "hub-fill")}, flush=True)


if __name__ == "__main__":
    main()
