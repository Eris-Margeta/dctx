#!/usr/bin/env python3
"""Offline tests for the independent benchmark (v1 card 6). Python 3 stdlib only; no network, no tool binaries.

python3 -m unittest discover -s docs/research/bench-independent/tests -v
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOP = os.path.dirname(HERE)
sys.path.insert(0, TOP)

import adapters  # noqa: E402
import gold_lsp  # noqa: E402
import lsp  # noqa: E402
import pick_repos  # noqa: E402
import questions  # noqa: E402
import report  # noqa: E402
import tokens  # noqa: E402

CFG = pick_repos.load(os.path.join(TOP, "repos.json"))
SEED = "a" * 40


class Selection(unittest.TestCase):
    def test_one_repo_per_language_by_the_published_rule(self):
        got = pick_repos.pick(CFG, SEED)
        self.assertEqual([g["lang"] for g in got], ["rust", "python", "go"])
        for g in got:
            cands = CFG["candidates"][g["lang"]]
            want = int(hashlib.sha256(f"{SEED}:{g['lang']}".encode()).hexdigest(), 16) % len(cands)
            self.assertEqual(g["index"], want)
            self.assertEqual(g["name"], cands[want]["name"])

    def test_deterministic_and_seed_sensitive(self):
        self.assertEqual(pick_repos.pick(CFG, SEED), pick_repos.pick(CFG, SEED))
        picks = {tuple(g["name"] for g in pick_repos.pick(CFG, hashlib.sha1(str(i).encode()).hexdigest())) for i in range(20)}
        self.assertGreater(len(picks), 5)

    def test_no_candidate_is_a_corpus_we_tuned_on(self):
        pick_repos.check_excluded(CFG)
        bad = json.loads(json.dumps(CFG))
        bad["candidates"]["python"].append({"name": "flask", "url": "https://github.com/pallets/flask.git", "tag": "3.1.3"})
        with self.assertRaises(ValueError):
            pick_repos.check_excluded(bad)

    def test_seed_must_be_a_full_commit_sha(self):
        for s in ("", "v1.0.0", "abc123", "g" * 40):
            with self.assertRaises(ValueError):
                pick_repos.pick(CFG, s)
        pick_repos.pick(CFG, "0" * 64)

    def test_pins_are_recorded(self):
        p = CFG["pins"]["tokenizer"]
        self.assertEqual((p["name"], p["version"]), ("cl100k_base", "0.8.0"))
        self.assertEqual(len(p["bpe_sha256"]), 64)
        for k in ("rust-analyzer", "pyright", "gopls"):
            self.assertIn(k, CFG["pins"])


class FakeServer(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        with open(os.path.join(self.root, "a.py"), "w") as f:
            f.write("def helper(x):\n    return x\n\nclass Box:\n    def method_x(self):\n        pass\n\n\ndef caller():\n    helper(1)\n    helper(2)\n")
        self.c = lsp.Client([sys.executable, os.path.join(HERE, "fake_lsp.py")], self.root)
        self.info = self.c.initialize()
        self.c.wait_ready(quiet=0.2, timeout=10)

    def tearDown(self):
        self.c.shutdown()

    def test_initialize_answers_server_requests_and_reports_server_info(self):
        self.assertEqual(self.info["serverInfo"]["name"], "fake")
        self.assertIn("srv1", self.c.answered)
        self.assertIn("srv2", self.c.answered)
        self.assertEqual(self.c.open_progress(), 0)

    def test_targets_and_gold_come_from_call_hierarchy(self):
        targets = gold_lsp.collect(self.c, self.root, ["a.py"], "python", SEED, n=5)
        self.assertEqual([t["def"]["name"] for t in targets], ["helper"])
        t = targets[0]
        self.assertEqual(t["def"], {"name": "helper", "file": "a.py", "line": 1, "end": 2, "kind": "function", "container": None})
        self.assertEqual([(s["file"], s["line"], s["caller"]) for s in t["callers"]], [("a.py", 10, "caller"), ("a.py", 11, "caller")])
        self.assertEqual(t["source"], "lsp:callHierarchy/incomingCalls")

    def test_callees_keep_only_in_repo_targets(self):
        item = gold_lsp.prepare(self.c, "a.py", self.root, 8, 0)
        self.assertEqual(gold_lsp.callees_of(self.c, item, self.root), [{"name": "helper", "file": "a.py", "line": 1}])

    def test_outline_is_top_level_symbols(self):
        syms = gold_lsp.file_symbols(self.c, self.root, "a.py", "python")
        self.assertEqual([s["name"] for s in syms if s["top"]], ["helper", "Box", "caller"])
        self.assertEqual([(s["name"], s["container"]) for s in syms if not s["top"]], [("method_x", "Box")])


class SymbolShapes(unittest.TestCase):
    def test_symbol_information_form(self):
        res = [{"name": "run", "kind": 12, "location": {"uri": "file:///r/x.go", "range": {"start": {"line": 4, "character": 5}, "end": {"line": 9, "character": 1}}},
                "containerName": ""}]
        s = gold_lsp.flatten_symbols(res)
        self.assertEqual((s[0]["name"], s[0]["line"], s[0]["end"], s[0]["sel_char"], s[0]["top"]), ("run", 5, 10, 5, True))

    def test_eligibility(self):
        ok = {"name": "parse_args", "kind": 12}
        self.assertTrue(gold_lsp.eligible(ok, "src/cli.py"))
        self.assertFalse(gold_lsp.eligible(dict(ok, name="main"), "src/cli.py"))
        self.assertFalse(gold_lsp.eligible(dict(ok, name="run"), "src/cli.py"))
        self.assertFalse(gold_lsp.eligible(dict(ok, name="test_parse"), "src/cli.py"))
        self.assertFalse(gold_lsp.eligible(ok, "tests/test_cli.py"))
        self.assertFalse(gold_lsp.eligible(ok, "src/cli_test.go"))
        self.assertFalse(gold_lsp.eligible(dict(ok, kind=5), "src/cli.py"))
        self.assertTrue(gold_lsp.eligible(dict(ok, kind=6), "src/cli.py"))

    def test_framing_round_trip(self):
        import io
        b = lsp.encode({"jsonrpc": "2.0", "id": 1, "result": "ü"})
        self.assertTrue(b.startswith(b"Content-Length: "))
        self.assertEqual(lsp.read_message(io.BytesIO(b))["result"], "ü")
        self.assertIsNone(lsp.read_message(io.BytesIO(b"")))


def fixture_gold(n=7):
    ts = []
    for i in range(n):
        ts.append({"def": {"name": f"func{i}", "file": f"src/m{i}.py", "line": 10 + i, "end": 20 + i, "kind": "function", "container": None},
                   "callers": [{"file": f"src/c{i}.py", "line": 5, "caller": f"use{i}", "caller_line": 3},
                               {"file": f"src/c{i}.py", "line": 9, "caller": f"use{i}", "caller_line": 3},
                               {"file": "src/z.py", "line": 2, "caller": "zed", "caller_line": 1}],
                   "callers2": [{"file": "src/top.py", "line": 7, "caller": "top", "caller_line": 6}],
                   "callees": [{"name": f"dep{i}", "file": "src/d.py", "line": 1}],
                   "source": "lsp:callHierarchy/incomingCalls"})
    return {"repo": "r", "lang": "python", "targets": ts,
            "outlines": {f"src/m{i}.py": [f"func{i}", f"Klass{i}"] for i in range(n)}}


class Questions(unittest.TestCase):
    def test_subject_cleaning(self):
        self.assertEqual(questions.clean_subject("Fix crash when path is empty (#123)"), "crash when path is empty")
        self.assertEqual(questions.clean_subject("fix(cli): handle unicode args, closes #44"), "handle unicode args")
        self.assertEqual(questions.clean_subject("Fixes #9: wrong exit code on timeout"), "wrong exit code on timeout")
        self.assertIsNone(questions.clean_subject("Merge pull request #5 from x/y"))
        self.assertIsNone(questions.clean_subject("Fix #12"))
        self.assertIsNone(questions.clean_subject("Bump version to 1.2"))

    def test_twenty_questions_over_three_repos(self):
        concepts = [{"words": f"concept words number {i}", "files": [f"src/m{i}.py"], "commit": "c" * 40, "subject": "s"} for i in range(3)]
        allq = []
        for idx in range(3):
            allq += questions.build(f"r{idx}", idx, fixture_gold(), concepts, SEED)
        self.assertEqual(len(allq), 20)
        self.assertEqual(len({q["id"] for q in allq}), 20)
        kinds = [q["kind"] for q in allq]
        for k in ("def", "callers", "callees", "outline", "impact", "concept"):
            self.assertIn(k, kinds)
        self.assertEqual(allq, sum((questions.build(f"r{i}", i, fixture_gold(), concepts, SEED) for i in range(3)), []))

    def test_facts_are_from_gold_not_tools(self):
        qs = questions.build("r0", 0, fixture_gold(), [{"words": "a b c", "files": ["src/x.py"], "commit": "c" * 40, "subject": "s"}] * 2, SEED)
        by = {}
        for q in qs:
            by.setdefault(q["kind"], q)
        d = by["def"]
        self.assertEqual(d["facts"], [["loc", [d["t"]["def"]["file"], d["t"]["def"]["line"]]]])
        c = by["callers"]["facts"]
        self.assertIn(["caller", ["src/z.py", [2]]], c)
        self.assertTrue(any(f[0] == "caller" and f[1][1] == [5, 9] for f in c))
        self.assertEqual(by["concept"]["facts"], [["path", "src/x.py"]])
        self.assertTrue(all(f[0] == "word" for f in by["outline"]["facts"]))
        self.assertIn(["path", "src/top.py"], by["impact"]["facts"])


class Scoring(unittest.TestCase):
    def test_judge_precision_recall(self):
        gold = [{"file": "a.py", "line": 10}, {"file": "a.py", "line": 11}, {"file": "b.py", "line": 3}]
        j = adapters.judge([("a.py", 9, 11), ("c.py", 1, 5)], gold)
        self.assertEqual((j["hits"], j["tp_hits"], j["found"], j["gold"]), (2, 1, 2, 3))

    def test_parsers(self):
        d = json.dumps({"records": [{"edge": "calls", "path": "a.py", "lines": [9, 11]}, {"edge": "imports", "path": "x"}], "next": "c1", "more": True})
        self.assertEqual(adapters.parse_dctx(d.encode()), ([("a.py", 9, 11)], "c1"))
        t = json.dumps({"matches": [{"symbol": {"path": "a.py", "span": "L1-L2"}, "hits": [{"relation": "calls", "path": "a.py", "span": "L10"}]},
                                    {"symbol": {"path": "z.py", "span": "L1-L2"}, "hits": [{"relation": "calls", "path": "q.py", "span": "L3"}]}]})
        self.assertEqual(adapters.parse_trail(t.encode(), "a.py", 1), [("a.py", 10, 10)])
        g = b"- caller [function] a.py:L10\nnoise\n"
        self.assertEqual(adapters.parse_gfy(g), [("a.py", 8, 12)])

    def test_question_score_rule(self):
        facts = [["path", "src/x.py"], ["word", "helper"]]
        self.assertEqual(adapters.score(facts, [b"see src/x.py: helper()"], ["answer"])[2], 2)
        self.assertEqual(adapters.score(facts, [b"helper"], ["answer"])[2], 1)
        self.assertEqual(adapters.score([["caller", ["a.py", [10]]]], [b"a.py:10"], ["find"])[2], 0)


class Tokens(unittest.TestCase):
    def test_bpe_hash_is_checked(self):
        with tempfile.NamedTemporaryFile(delete=False) as f:
            f.write(b"x")
        with self.assertRaises(RuntimeError):
            tokens.check_bpe(f.name, "0" * 64)
        tokens.check_bpe(f.name, hashlib.sha256(b"x").hexdigest())

    def test_missing_tiktoken_fails_loudly(self):
        saved = sys.modules.get("tiktoken")
        sys.modules["tiktoken"] = None
        try:
            with self.assertRaises(RuntimeError):
                tokens.Counter(CFG["pins"]["tokenizer"])
        finally:
            if saved is None:
                del sys.modules["tiktoken"]
            else:
                sys.modules["tiktoken"] = saved


class Report(unittest.TestCase):
    def test_table_lists_every_tool_and_both_measures(self):
        tasks = [{"tool": t, "repo": "r", "kind": "def", "score": s, "facts_n": 2, "facts_found": s, "tokens": 100} for t, s in
                 (("dctx", 2), ("trail", 1), ("graphify", 0))]
        callers = [{"tool": t, "repo": "r", "hits": 4, "tp_hits": 3, "found": 3, "gold": 6, "tokens": 50} for t in ("dctx", "trail", "graphify")]
        agg = report.aggregate(tasks, callers)
        self.assertEqual(agg["tasks"]["dctx"]["score"], 2)
        self.assertEqual(agg["callers"]["trail"]["precision"], 0.75)
        self.assertEqual(agg["callers"]["trail"]["recall"], 0.5)
        md = report.table(agg, {"selection": [{"name": "r", "lang": "python", "tag": "1", "sha": "s" * 40}], "seed": SEED, "tokenizer": "cl100k_base"})
        for w in ("| dctx |", "| trail |", "| graphify |", "Points", "Precision", "cl100k_base", SEED):
            self.assertIn(w, md)


class Scripts(unittest.TestCase):
    def test_shell_scripts_parse(self):
        for s in ("select.sh", "setup.sh", "run.sh"):
            p = os.path.join(TOP, s)
            self.assertTrue(os.path.isfile(p), s)
            self.assertEqual(subprocess.run(["sh", "-n", p]).returncode, 0, s)

    def test_select_sh_writes_selection_without_network(self):
        out = os.path.join(tempfile.mkdtemp(), "selection.json")
        r = subprocess.run(["sh", os.path.join(TOP, "select.sh"), "--seed", SEED, "--no-resolve", "--out", out], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        sel = json.load(open(out))
        self.assertEqual(sel["seed"], SEED)
        self.assertEqual([s["lang"] for s in sel["selection"]], ["rust", "python", "go"])
        self.assertEqual(sel["selection"], [dict(s, sha=None) for s in pick_repos.pick(CFG, SEED)])


if __name__ == "__main__":
    unittest.main()
