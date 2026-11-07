#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure a built dctx stdio server, using only the Python 3.11+ stdlib.

No model, client SDK, network, or ambient dctx cache is used. Temporary fixtures
and caches live under --scratch-dir (default: current directory) and are removed.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import statistics
import subprocess
import tempfile
import threading
import time


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class Server:
    """One session; drain stdout on a thread so reads have a portable timeout."""

    def __init__(self, binary, root, env, profile, timeout):
        self.timeout = timeout
        self.next_id = 0
        self.replies = queue.Queue()
        self.stderr = tempfile.TemporaryFile(dir=root.parent)
        self.process = subprocess.Popen(
            [str(binary), "mcp", "--profile", profile, str(root)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, env=env,
        )
        self.reader = threading.Thread(target=self._drain, daemon=True)
        self.reader.start()

    def _drain(self):
        for line in self.process.stdout:
            self.replies.put((line, time.perf_counter()))
        self.replies.put((b"", time.perf_counter()))

    def send(self, method, params=None, notify=False):
        request = {"jsonrpc": "2.0", "method": method}
        if not notify:
            self.next_id += 1
            request["id"] = self.next_id
        if params is not None:
            request["params"] = params
        self.process.stdin.write(compact(request) + b"\n")
        self.process.stdin.flush()
        return request.get("id")

    def receive(self, expected_id):
        try:
            line, received = self.replies.get(timeout=self.timeout)
        except queue.Empty as error:
            raise RuntimeError("timed out waiting for MCP reply") from error
        require(line, "MCP server exited before replying")
        reply = json.loads(line)
        require(reply.get("id") == expected_id, "unexpected MCP response order/id")
        require("error" not in reply, f"MCP protocol error: {reply.get('error')}")
        return reply["result"], line, received

    def request(self, method, params=None):
        return self.receive(self.send(method, params))

    def initialize(self):
        result = self.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dctx-mcp-measure", "version": "1"},
        })
        self.send("notifications/initialized", notify=True)
        return result

    def call(self, name, arguments):
        result, wire, received = self.request("tools/call", {
            "name": name, "arguments": arguments,
        })
        require(not result.get("isError"), f"tool failed: {result}")
        return result, wire, received

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=self.timeout if exc_type is None else 1)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.reader.join(timeout=1)
        self.process.stdout.close()
        self.stderr.seek(0)
        diagnostics = self.stderr.read().decode("utf-8", errors="replace")
        self.stderr.close()
        if exc_type is None:
            require(self.process.returncode == 0, f"server exit: {diagnostics}")


def fixture(root):
    root.mkdir()
    for file in range(24):
        source = "".join(
            f"pub fn beacon_{file:02}_{item:02}() -> usize {{\n    {item}\n}}\n\n"
            for item in range(20)
        )
        (root / f"part_{file:02}.rs").write_text(source, encoding="utf-8", newline="\n")
    # Quotes, backslashes, Unicode and newlines exercise JSON string escaping.
    source = 'pub fn receipt() -> &\'static str {\n    "héllo \\\"MCP\\\" \\\\ path"\n}\n'
    (root / "read.rs").write_text(source, encoding="utf-8", newline="\n")
    (root / "long.rs").write_text(
        "pub fn long_read() {\n" + "".join(
            f"    // source line {line:03}\n" for line in range(2, 182)
        ) + "}\n",
        encoding="utf-8", newline="\n",
    )
    files = sorted(root.glob("*.rs"))
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.name.encode("utf-8") + b"\0" + path.read_bytes())
    return {"files": len(files), "bytes": sum(p.stat().st_size for p in files),
            "sha256": digest.hexdigest()}


def text_of(result):
    return "".join(item["text"] for item in result["content"] if item["type"] == "text")


def page(server, name, arguments):
    result, _, _ = server.call(name, {**arguments, "json": True})
    return json.loads(text_of(result))


def measure(binary, work, samples, burst, timeout):
    root = work / "fixture"
    info = fixture(root)
    # Strip ambient overrides so private caches and fixture defaults are reproducible.
    env = {key: value for key, value in os.environ.items() if not key.startswith("DCTX_")}
    env.update(DCTX_CACHE_DIR=str(work / "cache"), DCTX_AUTO_CLEAN="0")
    version = subprocess.check_output([str(binary), "--version"], env=env, timeout=timeout)
    report = {"binary": version.decode().strip(), "platform": platform.platform(),
              "python": platform.python_version(), "samples": samples, "burst": burst,
              "fixture": info, "profiles": [], "wrapping": [], "timings": {}}
    for profile in ("default", "lean"):
        with Server(binary, root, env, profile, timeout) as server:
            _, initialize_wire, _ = server.initialize()
            result, list_wire, _ = server.request("tools/list")
            definitions = result["tools"]
            report["profiles"].append({
                "profile": profile, "tools": len(definitions),
                "definitions_bytes": len(compact(definitions)),
                "input_schemas_bytes": sum(len(compact(t["inputSchema"])) for t in definitions),
                "tools_list_wire_bytes": len(list_wire),
                "startup_wire_bytes": len(initialize_wire) + len(list_wire),
            })

    # This cache has not seen a query. Handshakes above do not index the fixture.
    started = time.perf_counter()
    with Server(binary, root, env, "lean", timeout) as server:
        server.initialize()
        server.request("tools/list")
        ready = time.perf_counter()
        server.call("find", {"query": "beacon", "limit": 5})
        first = time.perf_counter()
        warm = []
        for _ in range(samples):
            before = time.perf_counter()
            server.call("find", {"query": "beacon", "limit": 5})
            warm.append((time.perf_counter() - before) * 1000)
        report["timings"].update(
            spawn_handshake_ms=(ready - started) * 1000,
            first_empty_cache_call_ms=(first - ready) * 1000,
            spawn_through_first_call_ms=(first - started) * 1000,
            warm_call_ms=warm, warm_median_ms=statistics.median(warm),
        )
        for json_output in (False, True):
            arguments = {"id": "m:read.rs", "budget": 2000}
            cli = [str(binary), "map", "read", "m:read.rs", "--budget", "2000"]
            if json_output:
                arguments["json"] = True
                cli.append("--json")
            result, wire, _ = server.call("read", arguments)
            stdout = subprocess.check_output(cli, cwd=root, env=env, timeout=timeout)
            text = text_of(result).encode("utf-8")
            require(text == stdout, "MCP decoded text differs from CLI stdout")
            require(b"receipt" in stdout, "read did not return fixture source")
            report["wrapping"].append({
                "output": "facade JSON" if json_output else "text",
                "cli_stdout_bytes": len(stdout), "mcp_result_bytes": len(compact(result)),
                "mcp_wire_bytes": len(wire), "wire_ratio": len(wire) / len(stdout),
                "decoded_text_equals_cli": True,
            })

        # Multiple outstanding requests on ONE connection, then a ping behind them.
        before = time.perf_counter()
        ids = [server.send("tools/call", {
            "name": "find", "arguments": {"query": "beacon", "limit": 5},
        }) for _ in range(burst)]
        ping_id = server.send("ping")
        arrivals = []
        for request_id in ids:
            result, _, received = server.receive(request_id)
            require(not result.get("isError"), "burst tool failed")
            arrivals.append((received - before) * 1000)
        _, _, received = server.receive(ping_id)
        report["timings"].update(burst_completion_ms=arrivals,
                                 queued_ping_ms=(received - before) * 1000)

        # Exercise existing continuation plumbing instead of assuming pagination.
        report["pagination"] = {}
        for name, arguments in (("find", {"query": "beacon", "limit": 2}),
                                ("read", {"id": "m:long.rs", "budget": 400})):
            first_page = page(server, name, arguments)
            cursor = first_page.get("next")
            require(isinstance(cursor, str) and cursor.startswith("v1|"),
                    f"{name} did not supply a continuation: {first_page}")
            second_page = page(server, name, {**arguments, "cursor": cursor})
            require(first_page["records"] and second_page["records"], "empty page")
            require(first_page["records"] != second_page["records"], "cursor did not advance")
            report["pagination"][name] = {"first_records": len(first_page["records"]),
                                           "second_records": len(second_page["records"]),
                                           "cursor_advanced": True}
        window = page(server, "read", {"id": "m:long.rs", "lines": "35-37"})
        expected = "".join((root / "long.rs").read_text(encoding="utf-8").splitlines(True)[34:37])
        require(window["records"][0]["source_utf8"] == expected,
                "explicit absolute read window differs from fixture")
        report["pagination"]["absolute_read_window_verified"] = True
        result, _, _ = server.request("tools/call", {"name": "read", "arguments": {}})
        require(result.get("isError") is True, "tool error did not set isError")
        report["tool_error"] = result
        require(server.request("ping")[0] == {}, "session did not survive tool error")
    return report


def markdown(report):
    print(f"Binary: `{report['binary']}`; {report['platform']}; Python {report['python']}.")
    info = report["fixture"]
    print(f"\nFixture: {info['files']} Rust files, {info['bytes']:,} bytes; SHA-256 `{info['sha256']}`.")
    print("\nByte counts are UTF-8; wire counts include newline framing. Token estimates = bytes / 4.")
    print("\n| Profile | Tools | Tool-definition bytes | Est. tokens | Input-schema bytes | tools/list wire bytes | Initialize + list wire bytes | Est. startup tokens |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for row in report["profiles"]:
        print(f"| {row['profile']} | {row['tools']} | {row['definitions_bytes']:,} | "
              f"{row['definitions_bytes'] / 4:,.2f} | {row['input_schemas_bytes']:,} | "
              f"{row['tools_list_wire_bytes']:,} | {row['startup_wire_bytes']:,} | "
              f"{row['startup_wire_bytes'] / 4:,.2f} |")
    print("\n| Read output | CLI stdout bytes | MCP result bytes | MCP wire bytes | Wire / CLI |")
    print("| --- | ---: | ---: | ---: | ---: |")
    for row in report["wrapping"]:
        print(f"| {row['output']} | {row['cli_stdout_bytes']:,} | {row['mcp_result_bytes']:,} | "
              f"{row['mcp_wire_bytes']:,} | {row['wire_ratio']:.3f}× |")
    timing = report["timings"]
    print("\n| Wall-time measurement (lean) | Milliseconds |")
    print("| --- | ---: |")
    for label, key in (("Spawn + initialize + tools/list", "spawn_handshake_ms"),
                       ("First find, empty dctx cache", "first_empty_cache_call_ms"),
                       ("Spawn through first find", "spawn_through_first_call_ms"),
                       (f"Warm find median ({report['samples']} calls)", "warm_median_ms"),
                       (f"Pipelined {report['burst']} finds: first reply", "burst_completion_ms"),
                       ("Ping queued after burst", "queued_ping_ms")):
        value = timing[key]
        print(f"| {label} | {value[0] if isinstance(value, list) else value:.3f} |")
    print("\nBurst reply completion offsets (ms): " + ", ".join(
        f"{value:.3f}" for value in timing["burst_completion_ms"]) + ". Replies followed request order.")
    print("\nVerified: read text equals CLI stdout byte-for-byte in both formats; find/read cursors advance;")
    print("tool errors carry isError + text, and the same session answers a subsequent ping.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built dctx executable")
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--burst", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=60, help="per-reply/command seconds")
    parser.add_argument("--scratch-dir", type=Path, default=Path.cwd())
    parser.add_argument("--format", choices=("markdown", "json"), default="markdown")
    args = parser.parse_args()
    if args.samples < 1 or args.burst < 1 or args.timeout <= 0:
        parser.error("samples, burst and timeout must be positive")
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix=".dctx-mcp-measure-", dir=args.scratch_dir) as scratch:
        report = measure(binary, Path(scratch).resolve(), args.samples, args.burst, args.timeout)
    if args.format == "json":
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        markdown(report)


if __name__ == "__main__":
    main()
