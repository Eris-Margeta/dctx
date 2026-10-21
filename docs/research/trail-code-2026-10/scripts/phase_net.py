#!/usr/bin/env python3
"""Network-attempt audit: run each tool under strace inside the network namespace and log socket calls.

usage: phase_net.py CORPUS SYMBOL
Variants: Trail with telemetry gate open (DO_NOT_TRACK unset, update-check stamp removed) and with
DO_NOT_TRACK=1; dctx default. Nothing can leave the host: the namespace has only a down loopback.
"""
import os
import re
import sys

from benchlib import W, WORK, RES, run, as_bench, append_jsonl

HOME = os.path.join(W, "home")


def audit(tool, label, cmd, cwd, extra=None):
    trace = os.path.join(W, "runs", "tmp", f"strace.{tool}.{label}.txt")
    as_bench(f"rm -f '{trace}'")
    r = run(["strace", "-f", "-qq", "-e", "trace=socket,connect,sendto,sendmsg,bind", "-o", trace] + cmd, cwd,
            tool=tool, corpus="net", label=label, extra_env=extra, log="net_runs.jsonl")
    txt = as_bench(f"cat '{trace}' 2>/dev/null || true")
    connects = re.findall(r"connect\(\d+, (\{[^}]*\})", txt)
    inet = [c for c in connects if "AF_INET" in c]
    unix = [c for c in connects if "AF_UNIX" in c]
    socks = re.findall(r"socket\((AF_\w+)", txt)
    row = {"tool": tool, "label": label, "cmd": " ".join(cmd), "rc": r["rc"], "socket_calls": len(socks),
           "socket_families": sorted(set(socks)), "connect_inet": sorted(set(inet))[:20],
           "connect_unix": sorted(set(unix))[:10], "connect_total": len(connects)}
    append_jsonl("network.jsonl", row)
    with open(os.path.join(RES, f"strace.{tool}.{label}.txt"), "w") as f:
        f.write(txt[:200000])
    print(row, flush=True)


def main(corpus, sym):
    td, dd = os.path.join(WORK, "trail", corpus), os.path.join(WORK, "dctx", corpus)
    open_gate = {"DO_NOT_TRACK": ""}
    as_bench(f"cp -p '{HOME}/.graft/update-check.json' '{HOME}/.graft/update-check.json.keep' 2>/dev/null || true")
    as_bench(f"rm -f '{HOME}/.graft/update-check.json'")
    audit("trail", "callers_telemetry_on", ["trail", "callers", sym], td, open_gate)
    audit("trail", "build_telemetry_on", ["trail", "build", "."], td, open_gate)
    as_bench(f"rm -f '{HOME}/.graft/update-check.json'")
    audit("trail", "callers_dnt", ["trail", "callers", sym], td)
    audit("trail", "build_dnt", ["trail", "build", "."], td)
    audit("trail", "telemetry_status", ["trail", "telemetry", "status"], td, open_gate)
    audit("trail", "telemetry_debug", ["trail", "telemetry", "debug"], td, open_gate)
    as_bench(f"[ -f '{HOME}/.graft/update-check.json.keep' ] && mv '{HOME}/.graft/update-check.json.keep' '{HOME}/.graft/update-check.json' || true")
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    audit("dctx", "find", ["dctx", "map", "find", sym], dd, env)
    audit("dctx", "pack", ["dctx", "pack", "."], dd, env)
    audit("dctx", "plugin_list", ["dctx", "plugin", "list"], dd, env)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
