#!/usr/bin/env python3
"""A tiny stdio LSP server for the tests: one file a.py with `helper` (called twice by `caller`) and `caller`.

It asks the client for workspace/configuration and creates a progress token before it answers, as real servers do,
so the tests also cover the client's handling of server-to-client requests and readiness.
"""
import json
import sys

URI = None


def send(msg):
    b = json.dumps(msg).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
    sys.stdout.buffer.flush()


def read():
    n = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        k, _, v = line.decode().partition(":")
        if k.lower() == "content-length":
            n = int(v)
    return json.loads(sys.stdin.buffer.read(n))


def rng(a, b=None):
    b = a if b is None else b
    return {"start": {"line": a, "character": 0}, "end": {"line": b, "character": 1}}


def item(name, line, end):
    return {"name": name, "kind": 12, "uri": URI, "range": rng(line, end), "selectionRange": rng(line)}


def main():
    global URI
    while True:
        m = read()
        if m is None:
            return
        meth, mid, p = m.get("method"), m.get("id"), m.get("params") or {}
        if meth == "initialize":
            URI = p["rootUri"].rstrip("/") + "/a.py"
            send({"jsonrpc": "2.0", "id": "srv1", "method": "workspace/configuration", "params": {"items": [{"section": "x"}]}})
            send({"jsonrpc": "2.0", "id": "srv2", "method": "window/workDoneProgress/create", "params": {"token": "t"}})
            send({"jsonrpc": "2.0", "id": mid, "result": {"capabilities": {"callHierarchyProvider": True}, "serverInfo": {"name": "fake", "version": "0.1"}}})
        elif meth == "initialized":
            send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t", "value": {"kind": "begin", "title": "index"}}})
            send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t", "value": {"kind": "end"}}})
        elif meth == "textDocument/documentSymbol":
            send({"jsonrpc": "2.0", "id": mid, "result": [
                {"name": "helper", "kind": 12, "range": rng(0, 1), "selectionRange": rng(0), "children": []},
                {"name": "Box", "kind": 5, "range": rng(3, 6), "selectionRange": rng(3),
                 "children": [{"name": "method_x", "kind": 6, "range": rng(4, 5), "selectionRange": rng(4)}]},
                {"name": "caller", "kind": 12, "range": rng(8, 10), "selectionRange": rng(8)}]})
        elif meth == "textDocument/prepareCallHierarchy":
            ln = p["position"]["line"]
            send({"jsonrpc": "2.0", "id": mid, "result": [item({0: "helper", 8: "caller"}.get(ln, "x"), ln, ln + 1)]})
        elif meth == "callHierarchy/incomingCalls":
            res = []
            if p["item"]["name"] == "helper":
                res = [{"from": item("caller", 8, 10), "fromRanges": [rng(9), rng(10)]}]
            send({"jsonrpc": "2.0", "id": mid, "result": res})
        elif meth == "callHierarchy/outgoingCalls":
            res = []
            if p["item"]["name"] == "caller":
                res = [{"to": item("helper", 0, 1), "fromRanges": [rng(9)]},
                       {"to": dict(item("print", 0, 0), uri="file:///usr/lib/python3/builtins.pyi"), "fromRanges": [rng(10)]}]
            send({"jsonrpc": "2.0", "id": mid, "result": res})
        elif meth == "shutdown":
            send({"jsonrpc": "2.0", "id": mid, "result": None})
        elif meth == "exit":
            return
        elif mid is not None and meth:
            send({"jsonrpc": "2.0", "id": mid, "result": None})


if __name__ == "__main__":
    main()
