#!/usr/bin/env python3
"""Trail-Code MCP server probe: tool-definition size (paid by every agent session) and two tool calls.

usage: phase_mcp.py CORPUS SYMBOL FILE
dctx ships no MCP server at the measured commit (listed as planned in its README).
"""
import json
import os
import subprocess
import sys
import time

from benchlib import W, WORK, BENV, append_jsonl, est_tokens


def main(corpus, sym, file):
    d = os.path.join(WORK, "trail", corpus)
    p = subprocess.Popen([BENV, "timeout", "120", "trail", "mcp", "."], cwd=d, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    msgs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "bench", "version": "0"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
         "params": {"name": "trail_trace_calls", "arguments": {"symbol": sym}}},
        {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
         "params": {"name": "trail_file_api", "arguments": {"file": file}}},
    ]
    responses = {}
    t0 = time.monotonic()
    for m in msgs:
        p.stdin.write((json.dumps(m) + "\n").encode())
        p.stdin.flush()
        if "id" not in m:
            continue
        while True:
            line = p.stdout.readline()
            if not line:
                break
            try:
                j = json.loads(line)
            except ValueError:
                continue
            if j.get("id") == m["id"]:
                responses[m["id"]] = (line, time.monotonic() - t0)
                break
    p.stdin.close()
    try:
        p.wait(timeout=20)
    except subprocess.TimeoutExpired:
        p.kill()
    row = {"corpus": corpus}
    if 2 in responses:
        line, _ = responses[2]
        tools = json.loads(line)["result"]["tools"]
        defs = json.dumps(tools).encode()
        row.update(tools=[t["name"] for t in tools], tool_defs_bytes=len(defs), tool_defs_est_tokens=est_tokens(len(defs)),
                   per_tool_bytes={t["name"]: len(json.dumps(t)) for t in tools})
    for k, lab in ((3, "trace_calls"), (4, "file_api")):
        if k in responses:
            line, ts = responses[k]
            res = json.loads(line).get("result", {})
            text = "".join(c.get("text", "") for c in res.get("content", []))
            row[lab + "_bytes"] = len(text.encode())
            row[lab + "_est_tokens"] = est_tokens(len(text.encode()))
            row[lab + "_error"] = res.get("isError")
            row[lab + "_head"] = text[:300]
    row["stderr_tail"] = p.stderr.read().decode("utf-8", "replace")[-300:]
    append_jsonl("mcp.jsonl", row)
    print(json.dumps(row, indent=1))


if __name__ == "__main__":
    main(*sys.argv[1:4])
