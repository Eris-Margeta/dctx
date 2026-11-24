# SPDX-License-Identifier: Apache-2.0
"""Exercise the shell entry point with an offline GitHub API fixture."""
from base64 import b64encode
from io import BytesIO
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, unquote, urlsplit

SCRIPT = Path(__file__).resolve().with_name("require-green-ci.sh")
SHA = "a" * 40
TAG = "v1.2.3"
CREDENTIAL = "synthetic-release-credential"
API = "fixture:"
WORKFLOWS = ".github/workflows"
EARLIER = "2026-11-23T07:00:00Z"
LATER = "2026-11-23T09:00:00Z"
MAIN_PUSH = "on:\n  push:\n    branches: [main]\n  workflow_dispatch:\n"
CALLER = {"GITHUB_ACTIONS": "true", "GITHUB_RUN_ID": "700"}


def run(path, suite, conclusion="success", *, event="push", branch="main",
        created="2026-11-23T08:00:00Z", rid=None, attempt=1, status="completed"):
    return {
        "id": suite if rid is None else rid,
        "name": path,
        "path": f"{WORKFLOWS}/{path}",
        "event": event,
        "head_branch": branch,
        "head_sha": SHA,
        "status": status,
        "conclusion": conclusion,
        "check_suite_id": suite,
        "created_at": created,
        "run_attempt": attempt,
    }


def check(name, suite, conclusion="success", *, status="completed"):
    return {
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "check_suite": {"id": suite},
    }


def green():
    return {
        "policy": {"ci.yml": MAIN_PUSH, "go.yml": MAIN_PUSH, "npm.yml": MAIN_PUSH},
        "runs": [run("ci.yml@main", 1), run("go.yml", 2), run("npm.yml", 3)],
        "checks": [check("Linux tests", 1), check("Go transport", 2), check("RPC parity", 3)],
        "statuses": [],
    }


class Fake:
    def __init__(self, data):
        self.data = data

    def urlopen(self, request, *, timeout):
        url = urlsplit(request.full_url)
        prefix = "/repos/o/r/"
        assert url.scheme + ":" == API and not url.netloc and url.path.startswith(prefix), ascii(url)
        path = unquote(url.path.removeprefix(prefix))
        query = parse_qs(url.query)
        size = int(query.get("per_page", [30])[0])
        start = (int(query.get("page", [1])[0]) - 1) * size
        data = self.data

        auth = request.get_header("Authorization")
        if path == data.get("fail_path") and auth == f"Bearer {CREDENTIAL}":
            raise OSError(f'bad response\n\x1b"{auth}')
        if path in (f"commits/{TAG}", f"commits/{SHA}"):
            body = {"sha": data.get("resolved_sha", SHA)}
        elif path == f"contents/{WORKFLOWS}":
            assert query["ref"] == [SHA], query
            body = [{"type": "file", "path": f"{WORKFLOWS}/{name}"} for name in data["policy"]]
        elif path.startswith(f"contents/{WORKFLOWS}/"):
            assert query["ref"] == [SHA], query
            name = path.removeprefix(f"contents/{WORKFLOWS}/")
            body = {"encoding": "base64", "content": b64encode(data["policy"][name].encode()).decode()}
        elif path == "actions/runs":
            assert query["head_sha"] == [SHA], query
            body = {"workflow_runs": data["runs"][start:start + size]}
        elif path == "actions/runs/700":
            body = {"check_suite_id": data["own_suite"]}
        elif path == f"commits/{SHA}/check-runs":
            assert query["filter"] == ["latest"], query
            body = {"check_runs": data["checks"][start:start + size]}
        elif path == f"commits/{SHA}/status":
            statuses = data["statuses"]
            states = {item["state"] for item in statuses}
            if states & {"error", "failure"}:
                state = "failure"
            elif states == {"success"}:
                state = "success"
            else:
                state = "pending"
            body = {
                "state": data.get("aggregate", state),
                "total_count": len(statuses),
                "statuses": statuses[start:start + size],
            }
        else:
            raise AssertionError(f"unexpected request {request.full_url!r}")
        return BytesIO(json.dumps(body).encode())


def run_offline_python():
    # The shell still supplies the production Python program and arguments.
    assert sys.argv[2:4] == ["-I", "-"], sys.argv
    sys.argv = ["-", *sys.argv[4:]]
    fake = Fake(json.loads(os.environ["GUARD_TEST_API_DATA"]))
    with patch("urllib.request.urlopen", fake.urlopen), patch(
        "socket.socket", side_effect=AssertionError("unexpected socket access")
    ), patch("socket.getaddrinfo", side_effect=AssertionError("unexpected DNS lookup")):
        exec(compile(sys.stdin.read(), str(SCRIPT), "exec"), {"__name__": "__main__"})


class RequireGreenCi(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        tmp = TemporaryDirectory(prefix=".guard-test-", dir=SCRIPT.parent)
        cls.addClassCleanup(tmp.cleanup)
        cls.temp = Path(tmp.name)
        cls.script = cls.temp / SCRIPT.name
        shutil.copy2(SCRIPT, cls.script)
        cls.shim = cls.temp / "python3"
        cls.shim.write_text(
            "#!/usr/bin/env bash\n"
            'exec "$GUARD_TEST_PYTHON" -IB "$GUARD_TEST_DRIVER" --fake-api "$@"\n',
            encoding="utf-8",
        )
        cls.shim.chmod(0o755)

    def guard(self, data, env=None, ref=SHA, script=None):
        e = {
            key: value for key, value in os.environ.items()
            if key not in ("BASH_ENV", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_ACTIONS", "GITHUB_RUN_ID",
                           "GITHUB_WORKSPACE", "RUNNER_TEMP")
        }
        e.update({
            "PATH": f"{self.shim.parent}{os.pathsep}{e['PATH']}",
            "GITHUB_API_URL": API,
            "GUARD_TEST_PYTHON": sys.executable,
            "GUARD_TEST_DRIVER": str(Path(__file__).resolve()),
            "GUARD_TEST_API_DATA": json.dumps(data),
        })
        e.update(env or {})
        return subprocess.run(
            ["bash", str(script or self.script), ref, "o/r"], env=e, capture_output=True, text=True, timeout=10
        )

    def assert_green(self, result):
        self.assertEqual(result.returncode, 0, ascii(result.stderr))
        self.assertIn(f"ALL GREEN for {SHA}", result.stdout)

    def assert_refused(self, result, reason):
        self.assertEqual(result.returncode, 1, ascii(result.stdout))
        self.assertIn("REFUSED:", result.stderr)
        self.assertIn(reason, result.stderr)

    def test_all_green_with_no_status_contexts_passes(self):
        result = self.guard(green(), ref=TAG)
        self.assert_green(result)
        self.assertIn("0 statuses", result.stdout)

    def test_api_failure_does_not_disclose_release_credential(self):
        data = green()
        data["fail_path"] = f"commits/{SHA}/status"
        result = self.guard(data, {"GH_TOKEN": CREDENTIAL})
        self.assert_refused(result, "cannot read GitHub checks")
        for stream in (result.stdout, result.stderr):
            self.assertNotIn(CREDENTIAL, stream)
            self.assertNotIn("\x1b", stream)
        self.assertEqual(len(result.stderr.splitlines()), 1)

    def test_candidate_must_resolve_to_the_requested_full_sha(self):
        for resolved in ("main", "b" * 40):
            with self.subTest(resolved=resolved):
                data = dict(green(), resolved_sha=resolved)
                self.assert_refused(self.guard(data), "cannot resolve candidate commit")

    def test_local_copy_without_parser_gives_setup_instruction(self):
        with TemporaryDirectory(dir=self.temp) as tmp:
            shim = Path(tmp) / "python3"
            driver = shlex.join([sys.executable, "-ISB", str(Path(__file__).resolve()), "--fake-api"])
            shim.write_text(f'#!/bin/sh\nexec {driver} "$@"\n')
            shim.chmod(0o755)
            result = self.guard(green(), {"PATH": tmp + os.pathsep + os.environ["PATH"]})
            self.assert_refused(result, "PyYAML==6.0.2")
            self.assertFalse(result.stdout)

    def test_actions_provisions_before_use_and_reuses_successful_install(self):
        with TemporaryDirectory(dir=self.temp) as tmp:
            root = Path(tmp)
            checkout = root / "reviewed checkout"
            release = checkout / "tools" / "release"
            release.mkdir(parents=True)
            entry = release / SCRIPT.name
            lock = release / "ci-policy-requirements.txt"
            shutil.copy2(SCRIPT, entry)
            shutil.copy2(SCRIPT.with_name(lock.name), lock)
            # The stand-in records sanitized installer calls. Evaluation uses
            # the actual pinned parser and the same in-process API fixture.
            shim = root / "python3"
            driver = shlex.join([sys.executable, "-IB", str(Path(__file__).resolve())])
            shim.write_text(
                "#!/bin/sh\n"
                'if [ "$2" = -m ]; then\n'
                f'    exec {driver} --fake-install "$0" "$@"\n'
                "fi\n"
                f'[ "$0" != {shlex.quote(str(shim))} ] || exit 1\n'
                f'exec {driver} --fake-api "$@"\n'
            )
            shim.chmod(0o755)
            env = {
                "GITHUB_ACTIONS": "true", "GITHUB_WORKSPACE": str(checkout),
                "RUNNER_TEMP": tmp, "PATH": tmp + os.pathsep + os.environ["PATH"],
                "GH_TOKEN": CREDENTIAL, "GITHUB_TOKEN": CREDENTIAL,
            }
            fail = root / "fail-install"
            fail.touch()
            result = self.guard(green(), env, script=entry)
            self.assert_refused(result, "could not provision")
            self.assertNotIn("installer output", result.stdout + result.stderr)
            fail.unlink()
            for _ in range(2):
                self.assert_green(self.guard(green(), env, script=entry))
            calls = [json.loads(line) for line in (root / "installs").read_text().splitlines()]
            self.assertEqual([args[2] for args in calls], ["venv", "pip", "venv", "pip"])
            args = calls[-1]
            self.assertIn("--require-hashes", args)
            self.assertIn("--only-binary=:all:", args)
            self.assertIn("--no-deps", args)
            self.assertEqual(args[args.index("-r") + 1], str(lock))
            self.assertIn("PyYAML==6.0.2", lock.read_text())
            self.assertRegex(lock.read_text(), r"--hash=sha256:[a-f0-9]{64}")

    def test_red_non_ci_workflow_refuses(self):
        data = green()
        data["runs"][2] = run("npm.yml", 3, "failure")
        self.assert_refused(self.guard(data), "main push .github/workflows/npm.yml=completed/failure")

    def test_red_or_skipped_check_refuses(self):
        for conclusion in ("failure", "skipped"):
            with self.subTest(conclusion=conclusion):
                data = green()
                data["checks"][1] = check("Go transport", 2, conclusion)
                self.assert_refused(self.guard(data), f'check "Go transport"=completed/{conclusion}')

    def test_check_without_suite_still_counts(self):
        data = green()
        data["checks"].append(check("External", None, "failure"))
        self.assert_refused(self.guard(data), 'check "External"=completed/failure')

    def test_failed_push_refuses_with_green_filtered_dispatch_in_either_order(self):
        for dispatched in (EARLIER, LATER):
            with self.subTest(dispatched=dispatched):
                data = green()
                data["runs"][0] = run("ci.yml@main", 1, "failure")
                data["runs"].append(run("ci.yml@v1.2.3", 8, event="workflow_dispatch", created=dispatched))
                data["checks"].append(check("Filtered Linux tests", 8))
                self.assert_refused(self.guard(data), "main push .github/workflows/ci.yml=completed/failure")

    def test_green_push_and_newer_green_dispatch_pass(self):
        data = green()
        data["runs"].append(run("ci.yml", 8, event="workflow_dispatch", created=LATER))
        data["checks"].append(check("Filtered Linux tests", 8))
        self.assert_green(self.guard(data))

    def test_red_latest_dispatch_refuses_even_with_green_push(self):
        data = green()
        data["runs"].append(run("ci.yml", 8, "failure", event="workflow_dispatch", created=LATER))
        self.assert_refused(self.guard(data), "ci.yml=completed/failure")

    def test_non_main_push_dispatch_and_wrong_sha_cannot_supply_ci_push(self):
        for ci_run in (
            run("ci.yml", 1, branch="topic"),
            run("ci.yml", 1, event="workflow_dispatch"),
            dict(run("ci.yml", 1), head_sha="b" * 40),
        ):
            with self.subTest(ci_run=ci_run):
                data = green()
                data["runs"][0] = ci_run
                self.assert_refused(self.guard(data), "missing main push run for .github/workflows/ci.yml")

    def test_missing_required_workflow_refuses_even_with_dispatch(self):
        for runs in ([], [run("publish-check.yaml", 8, event="workflow_dispatch")]):
            with self.subTest(runs=runs):
                data = green()
                data["policy"]["publish-check.yaml"] = '"on": [push, pull_request]\n'
                data["runs"].extend(runs)
                self.assert_refused(self.guard(data, ref=TAG),
                                    "missing main push run for .github/workflows/publish-check.yaml")

    def test_policy_filters_main_but_does_not_require_other_branches_or_tags(self):
        data = green()
        data["policy"].update({
            "other.yml": "on: {push: {branches: ['m*', '!main']}}\n",
            "ignored.yml": "on: {push: {branches-ignore: [main]}}\n",
            "tag-only.yml": "on: {push: {tags-ignore: ['old-*']}}\n",
            "also-main.yml": "on: {push: {branches: ['m*', '!main', '**/mai[n]', '!ma**/in'], tags: ['v*']}}\n",
            "unrestricted.yml": "on: {push: {branches-ignore: ['old/*']}}\n",
        })
        self.assert_refused(self.guard(data), "missing main push run for .github/workflows/also-main.yml")
        self.assert_refused(self.guard(data), "missing main push run for .github/workflows/unrestricted.yml")
        data["runs"].extend([run("also-main.yml", 8), run("unrestricted.yml", 9)])
        self.assert_green(self.guard(data))

    def test_candidate_policy_accepts_block_and_flow_trigger_forms_with_pinned_parser(self):
        for policy in (
            "on: push\n",
            "on:\n- workflow_dispatch\n- 'push' # full push\n",
            "'on':\n  'push':\n    branches:\n    - 'm*'\n    - '!m#old' # exclude one branch\n",
            "on: {\npush: {branches: [\n  'old,*', \"main\",\n]},\nworkflow_dispatch: {},\n}\n",
            "on:\n  push: {}\n  workflow_dispatch:\n    inputs: {start: {default: quality}}\n",
        ):
            with self.subTest(policy=policy):
                data = green()
                data["policy"]["new.yml"] = policy
                self.assert_refused(self.guard(data), "missing main push run for .github/workflows/new.yml")
                data["runs"].append(run("new.yml", 8))
                self.assert_green(self.guard(data))

    def test_multiline_quoted_branch_filters(self):
        data = green()
        data["policy"]["go.yml"] = 'on: {push: {branches: ["ma\\\n  in"]}}'
        data["policy"]["other.yml"] = "on:\n  push:\n    branches: ['ma\n      in']\n"
        self.assert_green(self.guard(data))
        data["runs"].pop(1)
        self.assert_refused(self.guard(data), "missing main push run for .github/workflows/go.yml")

    def test_duplicate_keys_and_unsafe_tags_refuse(self):
        for policy in ("on: push\non: workflow_dispatch", "on: !!python/name:builtins.str ''"):
            with self.subTest(policy=policy):
                data = green()
                data["policy"]["new.yml"] = policy
                self.assert_refused(self.guard(data), "cannot read workflow trigger")

    def test_yaml_escapes_in_branch_filters_preserve_required_push(self):
        data = green()
        data["policy"]["go.yml"] = r'on: {push: {branches: ["m\x61in"]}}'
        data["policy"]["npm.yml"] = r'on: {push: {branches: ["\u006dai\U0000006e", "!ma\x0ain"]}}'
        data["policy"]["other.yml"] = r'on: {push: {branches: ["topic\_name", "m\\x61in"]}}'
        self.assert_green(self.guard(data))
        data["runs"] = data["runs"][:1]
        result = self.guard(data)
        for path in ("go.yml", "npm.yml"):
            self.assert_refused(result, f"missing main push run for {WORKFLOWS}/{path}")

    def test_folded_main_branch_requires_a_green_push(self):
        data = green()
        data["policy"]["extra.yml"] = (
            "on:\n  push:\n    branches:\n      - release/**\n"
            "      - >- # strip the final newline\n        main\n\n"
            "      - '!topic'\n  workflow_dispatch:\n"
        )
        self.assert_refused(self.guard(data), f"missing main push run for {WORKFLOWS}/extra.yml")
        data["runs"].append(run("extra.yml", 8))
        self.assert_green(self.guard(data))

    def test_folded_event_scalars_select_main_push_workflows(self):
        data = green()
        data["policy"]["dispatch.yml"] = "on: >-\n  workflow_dispatch\njobs: {}\n"
        data["policy"]["go.yml"] = "on:\n  - >-\n    workflow_dispatch\n  - >2-\n    push\n"
        data["policy"]["npm.yml"] = "on:\n  >\n  push"  # No final line break to clip.
        self.assert_green(self.guard(data))
        data["runs"] = data["runs"][:1]
        result = self.guard(data)
        for name in ("go.yml", "npm.yml"):
            self.assert_refused(result, f"missing main push run for {WORKFLOWS}/{name}")

    def test_folded_branch_content_preserves_whitespace_and_hashes(self):
        data = green()
        # None of these scalars decodes to "main". The final exclusion leaves
        # main required if the indented hash line is correctly read as content.
        data["policy"]["other.yml"] = (
            "on:\n  push:\n    branches:\n"
            "      - >-\n        ma\n        in\n"
            "      - >-\n        ma\n\n        in\n"
            "      - >\n        main\n"
        )
        data["policy"]["go.yml"] = (
            "on:\n  push:\n    branches:\n      - main\n"
            "      - >-\n        !main\n        # part of the pattern\n"
        )
        self.assert_green(self.guard(data))
        data["runs"].pop(1)
        self.assert_refused(self.guard(data), f"missing main push run for {WORKFLOWS}/go.yml")

    def test_invalid_yaml_escapes_refuse(self):
        for branch in (r"m\x6in", r"ma\qin", r"\U00110000"):
            with self.subTest(branch=branch):
                data = green()
                data["policy"]["go.yml"] = rf'on: {{push: {{branches: ["{branch}"]}}}}'
                self.assert_refused(self.guard(data), "cannot read workflow trigger for .github/workflows/go.yml")

    def test_anchored_main_push_filters_still_require_the_workflow(self):
        for policy in (
            "on:\n  push: &filters\n    branches: [main]\n  pull_request: *filters\n",
            "on:\n  pull_request: &filters {branches: [main]}\n  push: *filters\n",
            "on: {pull_request: {branches: &refs [main]}, push: {branches: *refs}}\n",
            "name: &primary main\njobs:\n  build:\n    strategy:\n      matrix:\n"
            "        refs: &branches [*primary]\non: {push: {branches: *branches}}\n",
            "env: {PRIMARY: &primary main}\non:\n  push:\n    branches:\n      - *primary\n",
        ):
            with self.subTest(policy=policy):
                data = green()
                data["policy"]["go.yml"] = policy
                self.assert_green(self.guard(data))
                data["runs"].pop(1)
                self.assert_refused(self.guard(data), "missing main push run for .github/workflows/go.yml")

    def test_aliased_branch_filter_excluding_main_allows_absent_workflow(self):
        data = green()
        data["policy"]["go.yml"] = (
            "on:\n  pull_request:\n    branches: &refs\n      - 'release/**'\n"
            "  push:\n    branches: *refs\n"
        )
        data["runs"].pop(1)
        self.assert_green(self.guard(data))

    def test_workflow_filename_containing_yml_at_is_distinct(self):
        data = green()
        compat = "ci.yml@compat.yml"
        data["policy"][compat] = MAIN_PUSH
        for path in (compat, compat + "@refs/heads/main", compat + "@topic.yml@next"):
            with self.subTest(path=path):
                data["runs"].append(run(path, 8))
                self.assert_green(self.guard(data))
                ci = data["runs"].pop(0)
                self.assert_refused(self.guard(data), "missing main push run for .github/workflows/ci.yml")
                data["runs"].insert(0, ci)
                data["runs"].pop()
        self.assert_refused(self.guard(data), "missing main push run for .github/workflows/" + compat)

    def test_undefined_trigger_alias_refuses(self):
        data = green()
        data["policy"]["new.yml"] = "on:\n  push:\n    branches: *shared\n"
        self.assert_refused(self.guard(data), "cannot read workflow trigger for .github/workflows/new.yml")

    def test_older_workflow_layout_accepts_reusable_path_filtered_and_tag_only_files(self):
        data = green()
        data["policy"].update({
            "go.yml": "on: [workflow_call, workflow_dispatch]\n",
            "npm.yml": "on: workflow_call\n",
            "deploy-site.yml": "on: {push: {branches: [main], paths: ['website/**']}}\n",
            "release.yml": "on: {push: {tags: ['v*']}}\n",
            ".ci.example": MAIN_PUSH,
        })
        data["runs"] = data["runs"][:1]
        data["checks"] = [check("Go / transport", 1), check("npm / RPC parity", 1)]
        self.assert_green(self.guard(data, ref=TAG))

    def test_new_push_replaces_failed_push_and_its_suite(self):
        data = green()
        data["runs"].append(run("ci.yml@main", 8, "failure", created=EARLIER))
        data["checks"].append(check("Previous Linux tests", 8, "failure"))
        self.assert_green(self.guard(data))

    def test_new_unfinished_push_replaces_green_push(self):
        data = green()
        data["runs"].append(run("ci.yml", 8, None, created=LATER, status="in_progress"))
        self.assert_refused(self.guard(data), "main push .github/workflows/ci.yml=in_progress/None")

    def test_rerun_uses_latest_attempt_and_keeps_its_suite(self):
        data = green()
        data["runs"][0] = run("ci.yml", 1, attempt=2)
        data["runs"].append(run("ci.yml", 1, "failure"))
        self.assert_green(self.guard(data))
        data["checks"][0] = check("Linux tests", 1, "failure")
        self.assert_refused(self.guard(data), 'check "Linux tests"=completed/failure')

    def test_dispatch_replacement_still_checks_the_required_push_suite(self):
        data = green()
        data["runs"].append(run("ci.yml@v1.2.3", 8, event="workflow_dispatch", created=LATER))
        data["checks"][0] = check("Linux tests", 1, "failure")
        self.assert_refused(self.guard(data), 'check "Linux tests"=completed/failure')

    def test_replaced_non_push_suite_is_ignored(self):
        data = green()
        data["runs"].append(run("go.yml@topic", 9, "failure", event="workflow_dispatch", created=EARLIER))
        data["checks"].append(check("Go transport", 9, "failure"))
        self.assert_green(self.guard(data))

    def test_unlisted_workflow_replacement_matches_api_workflow_id(self):
        data = green()
        data["runs"].extend([
            dict(run("external.yml@old", 7, "failure", created=EARLIER), workflow_id=42),
            dict(run("external.yml@new", 8, created=LATER), workflow_id=42),
        ])
        data["checks"].append(check("Previous external run", 7, "failure"))
        self.assert_green(self.guard(data))

    def test_caller_run_and_its_reusable_job_checks_are_ignored(self):
        data = green()
        data["runs"].append(run("release.yml@v1.2.3", 7, None, branch=TAG, rid=700, status="in_progress"))
        data["checks"].append(check("reproducibility / compare", 7, None, status="in_progress"))
        data["own_suite"] = 7
        self.assert_refused(self.guard(data), "release.yml@v1.2.3=in_progress/None")
        self.assert_green(self.guard(data, CALLER))
        data["runs"].pop()
        self.assert_green(self.guard(data, CALLER))
        data["checks"].append(check("reproducibility / compare", 8, "failure"))
        self.assert_refused(self.guard(data, CALLER), 'check "reproducibility / compare"=completed/failure')

    def test_caller_does_not_supply_a_required_main_push(self):
        data = green()
        data["runs"][0] = run("ci.yml", 1, rid=700)
        data["own_suite"] = 1
        self.assert_refused(self.guard(data, CALLER), "missing main push run for .github/workflows/ci.yml")

    def test_later_run_and_check_pages_contribute_to_verdict(self):
        data = green()
        data["runs"] = [run("extra.yml", i) for i in range(10, 110)] + data["runs"]
        data["checks"] = [check("External", None) for _ in range(100)] + data["checks"]
        self.assert_green(self.guard(data))
        data["checks"][-1] = check("Last check", 3, "failure")
        self.assert_refused(self.guard(data), 'check "Last check"=completed/failure')

    def test_red_aggregate_refuses_even_when_listed_context_is_green(self):
        data = green()
        data["statuses"] = [{"context": "external", "state": "success"}]
        data["aggregate"] = "pending"
        self.assert_refused(self.guard(data), "combined status=pending")

    def test_status_context_refuses(self):
        data = green()
        data["statuses"] = [{"context": "external", "state": "failure"}]
        self.assert_refused(self.guard(data), 'status "external"=failure')

    def test_refusal_escapes_workflow_command_text_and_line_breaks(self):
        data = green()
        data["statuses"] = [{
            "context": "external\n\x1b\"::warning::injected\r##[warning]injected\u2028end",
            "state": "failure",
        }]
        result = self.guard(data)
        self.assert_refused(result, r'external\n\u001b\"\x3a\x3awarning\x3a\x3ainjected\r\x23\x23[warning]injected\u2028end')
        self.assertEqual(len(result.stderr.splitlines()), 1, ascii(result.stderr))
        self.assertNotIn("::", result.stderr)
        self.assertNotIn("##[", result.stderr)

    def test_combined_status_honors_contexts_beyond_first_page(self):
        data = green()
        data["statuses"] = [{"context": f"external-{i}", "state": "success"} for i in range(101)]
        result = self.guard(data)
        self.assert_green(result)
        self.assertIn("101 statuses", result.stdout)
        for state, combined in (("pending", "pending"), ("error", "failure")):
            with self.subTest(state=state):
                data["statuses"][-1]["state"] = state
                result = self.guard(data)
                self.assert_refused(result, f"combined status={combined}")
                self.assertIn('status "external-100"=', result.stderr)


def run_offline_installer():
    executable = Path(sys.argv[2])
    args = sys.argv[3:]
    assert args[:2] == ["-I", "-m"]
    assert not ({"GH_TOKEN", "GITHUB_TOKEN"} & os.environ.keys())
    assert CREDENTIAL not in os.environ.values() and CREDENTIAL not in args
    if args[2] == "venv":
        root = executable.parent
        python = Path(args[3]) / "bin" / "python"
        python.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(executable, python)
    else:
        assert args[2] == "pip"
        root = executable.parents[2]
    with (root / "installs").open("a") as stream:
        print(json.dumps(args), file=stream)
    if args[2] == "pip" and (root / "fail-install").exists():
        print('installer output\n\x1b"', file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if sys.argv[1:2] == ["--fake-api"]:
        run_offline_python()
    elif sys.argv[1:2] == ["--fake-install"]:
        run_offline_installer()
    else:
        unittest.main()
