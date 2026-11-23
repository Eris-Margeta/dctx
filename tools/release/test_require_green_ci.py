# SPDX-License-Identifier: Apache-2.0
"""Offline tests for require-green-ci.sh against a fake GitHub API (no network)."""
import json
import os
import subprocess
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import urlparse

SCRIPT = Path(__file__).with_name("require-green-ci.sh")
SHA = "a" * 40


def run(path, name, suite, conclusion="success", event="push", created="2026-11-23T08:00:00Z", rid=None):
    return {"id": rid or suite, "name": name, "path": f".github/workflows/{path}", "event": event,
            "head_sha": SHA, "status": "completed", "conclusion": conclusion,
            "check_suite_id": suite, "created_at": created}


def check(name, suite, conclusion="success"):
    return {"name": name, "status": "completed", "conclusion": conclusion, "check_suite": {"id": suite}}


def green():
    return {
        "runs": [run("ci.yml", "CI", 1), run("go.yml", "Go package", 2), run("npm.yml", "npm transport", 3)],
        "checks": [check("Linux tests", 1), check("Go transport", 2), check("RPC parity", 3)],
        "statuses": [],
    }


class Fake(BaseHTTPRequestHandler):
    data = {}

    def log_message(self, *args):
        pass

    def do_GET(self):
        path = urlparse(self.path).path
        d = self.data
        if path.endswith(f"/commits/{SHA}"):
            body = {"sha": SHA}
        elif path.endswith("/actions/runs"):
            body = {"workflow_runs": d["runs"]}
        elif path.endswith("/check-runs"):
            body = {"check_runs": d["checks"]}
        elif path.endswith("/status"):
            body = {"statuses": d["statuses"]}
        elif "/actions/runs/" in path:
            body = {"check_suite_id": d.get("own_suite")}
        else:
            self.send_response(404)
            self.end_headers()
            return
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


class RequireGreenCi(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = HTTPServer(("127.0.0.1", 0), Fake)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def guard(self, data, env=None):
        Fake.data = data
        e = {k: v for k, v in os.environ.items() if k not in ("GH_TOKEN", "GITHUB_TOKEN", "GITHUB_ACTIONS", "GITHUB_RUN_ID")}
        e["GITHUB_API_URL"] = f"http://127.0.0.1:{self.server.server_port}"
        e.update(env or {})
        return subprocess.run(["bash", str(SCRIPT), SHA, "o/r"], env=e, capture_output=True, text=True)

    def test_all_green_passes(self):
        r = self.guard(green())
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("ALL GREEN", r.stdout)

    def test_one_red_non_ci_check_refuses(self):
        d = green()
        d["checks"][1] = check("Go transport", 2, "failure")
        r = self.guard(d)
        self.assertEqual(r.returncode, 1)
        self.assertIn('check "Go transport"=completed/failure', r.stderr)

    def test_red_non_ci_workflow_refuses(self):
        d = green()
        d["runs"][2] = run("npm.yml", "npm transport", 3, "failure")
        self.assertEqual(self.guard(d).returncode, 1)

    def test_skipped_check_and_red_status_refuse(self):
        d = green()
        d["checks"].append(check("deploy", 1, "skipped"))
        self.assertEqual(self.guard(d).returncode, 1)
        d = green()
        d["statuses"] = [{"context": "external", "state": "failure"}]
        self.assertEqual(self.guard(d).returncode, 1)

    def test_no_ci_push_run_refuses(self):
        d = green()
        d["runs"] = d["runs"][1:]
        self.assertEqual(self.guard(d).returncode, 1)

    def test_replaced_failed_run_is_ignored(self):
        d = green()
        d["runs"].append(run("go.yml", "Go package", 9, "failure", "workflow_dispatch", "2026-11-22T08:00:00Z"))
        d["checks"].append(check("Go transport", 9, "failure"))
        r = self.guard(d)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_own_running_release_run_is_ignored(self):
        d = green()
        d["runs"].append(dict(run("release.yml", "Release", 7, None, rid=700), status="in_progress"))
        d["checks"].append(dict(check("preflight", 7, None), status="in_progress"))
        d["own_suite"] = 7
        self.assertEqual(self.guard(d).returncode, 1)
        r = self.guard(d, {"GITHUB_ACTIONS": "true", "GITHUB_RUN_ID": "700"})
        self.assertEqual(r.returncode, 0, r.stderr)


if __name__ == "__main__":
    unittest.main()
