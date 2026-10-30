#!/usr/bin/env python3
"""Rebuild the October task questions on the pinned October corpora and check them against the stored ones.
usage: gen_questions.py ROOT   (ROOT = bench3 root; uses ROOT/corpora-oct, ROOT/tasks/oct)"""
import json, os, subprocess, sys
root = sys.argv[1]
oct_s = os.path.join(root, "tasks", "oct", "scripts")
sys.path.insert(0, oct_s)
import phase_tasks as pt
from goldlib import Corpus
CORP = os.path.join(root, "corpora-oct")
OT = os.path.join(root, "tasks", "oct", "targets")
OUT = os.path.join(root, "tasks", "questions")
PRIV = os.path.join(root, "tasks", "private")
os.makedirs(OUT, exist_ok=True); os.makedirs(PRIV, exist_ok=True)

def dump(qs):
    return json.loads(json.dumps([{k: v for k, v in q.items() if k != "t"} | {"target": q["t"]["def"]["name"]} if "t" in q else q for q in qs]))

summary = {}
for corpus in ["flask", "ripgrep", "pocketbase", "trailcode", "dctx", "django", "core"]:
    c = Corpus(os.path.join(CORP, corpus))
    tp = os.path.join(OT, f"{corpus}.targets.json")
    if corpus == "core":
        subprocess.run([sys.executable, os.path.join(oct_s, "select_targets.py"), CORP, PRIV, "core"], check=True, cwd=oct_s)
        tp = os.path.join(PRIV, "core.targets.json")
    targets = json.load(open(tp))["targets"]
    qs = pt.build_questions(corpus, c, targets)
    got = dump(qs)
    old = json.load(open(os.path.join(OT, f"{corpus}.questions.json")))
    if corpus == "core":
        exp = [(x["id"], x["kind"], x["facts_n"]) for x in old["data"]]
        have = [(q["id"], q["kind"], len(q["facts"])) for q in got]
        summary[corpus] = {"match_fact_counts": exp == have, "expected": exp, "have": have if exp != have else "same"}
        json.dump(got, open(os.path.join(PRIV, "core.questions.json"), "w"), indent=1)
    else:
        # json round trip turns tuples into lists
        same = got == old
        summary[corpus] = {"identical_to_october": same}
        if not same:
            for a, b in zip(got, old):
                if a != b:
                    summary[corpus].setdefault("diff_ids", []).append(a["id"])
        json.dump(got, open(os.path.join(OUT, f"{corpus}.questions.json"), "w"), indent=1)
print(json.dumps(summary, indent=1))
