#!/usr/bin/env python3
"""Minimal stdio LSP client for the gold generator (Python 3 stdlib only).

Answers the server-to-client requests real servers send (workspace/configuration, window/workDoneProgress/create,
client/registerCapability) and tracks $/progress tokens, so `wait_ready` can wait until indexing is over.
"""
import json
import os
import subprocess
import threading
import time
from urllib.parse import quote, unquote, urlparse


def encode(msg):
    """One JSON-RPC message with its Content-Length header."""
    b = json.dumps(msg, ensure_ascii=False).encode("utf-8")
    return b"Content-Length: %d\r\n\r\n" % len(b) + b


def read_message(stream):
    """Next message from a binary stream, or None at end of stream."""
    n = None
    while True:
        line = stream.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        k, _, v = line.decode("ascii", "replace").partition(":")
        if k.strip().lower() == "content-length":
            n = int(v.strip())
    if n is None:
        return None
    return json.loads(stream.read(n).decode("utf-8"))


def path_uri(path):
    """file:// URI of an absolute path."""
    return "file://" + quote(os.path.abspath(path))


def uri_path(uri):
    """Absolute path of a file:// URI."""
    return unquote(urlparse(uri).path)


class Client:
    """One language server process rooted at `root`. `settings` answers every workspace/configuration item."""

    def __init__(self, argv, root, env=None, settings=None, init_options=None, log=None):
        self.root, self.settings, self.init_options = os.path.abspath(root), settings or {}, init_options
        self.p = subprocess.Popen(argv, cwd=self.root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=log if log is not None else subprocess.DEVNULL)
        self.cv = threading.Condition()
        self.results, self.answered, self.progress = {}, set(), {}
        self.last = time.monotonic()
        self.next_id = 0
        self.wlock = threading.Lock()
        threading.Thread(target=self._reader, daemon=True).start()

    def _send(self, msg):
        with self.wlock:
            self.p.stdin.write(encode(msg))
            self.p.stdin.flush()

    def _reader(self):
        while True:
            try:
                m = read_message(self.p.stdout)
            except (ValueError, OSError):
                m = None
            with self.cv:
                if m is None:
                    self.results["__eof__"] = True
                    self.cv.notify_all()
                    return
                self.last = time.monotonic()
                if "method" in m and "id" in m:
                    self._answer(m)
                elif "method" in m:
                    if m["method"] == "$/progress":
                        p = m.get("params") or {}
                        kind = (p.get("value") or {}).get("kind")
                        if kind == "begin":
                            self.progress[str(p.get("token"))] = True
                        elif kind == "end":
                            self.progress.pop(str(p.get("token")), None)
                else:
                    self.results[m.get("id")] = m
                self.cv.notify_all()

    def _answer(self, m):
        meth, p = m["method"], m.get("params") or {}
        res = None
        if meth == "workspace/configuration":
            res = [self.settings.get(i.get("section") or "", self.settings.get("*", {})) for i in p.get("items", [])]
        self.answered.add(m["id"])
        threading.Thread(target=self._send, args=({"jsonrpc": "2.0", "id": m["id"], "result": res},), daemon=True).start()

    def request(self, method, params, timeout=300):
        """Send a request and wait for its result; raises RuntimeError on an error reply, EOF or timeout."""
        with self.cv:
            self.next_id += 1
            mid = self.next_id
        self._send({"jsonrpc": "2.0", "id": mid, "method": method, "params": params})
        end = time.monotonic() + timeout
        with self.cv:
            while mid not in self.results:
                if self.results.get("__eof__"):
                    raise RuntimeError(f"server exited during {method}")
                left = end - time.monotonic()
                if left <= 0:
                    raise RuntimeError(f"timeout after {timeout}s on {method}")
                self.cv.wait(left)
            m = self.results.pop(mid)
        if "error" in m:
            raise RuntimeError(f"{method}: {m['error']}")
        return m.get("result")

    def notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def initialize(self):
        caps = {"textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
                                 "callHierarchy": {"dynamicRegistration": False},
                                 "references": {"dynamicRegistration": False},
                                 "synchronization": {"didSave": False}},
                "workspace": {"configuration": True, "workspaceFolders": True},
                "window": {"workDoneProgress": True}}
        params = {"processId": os.getpid(), "rootUri": path_uri(self.root), "rootPath": self.root, "capabilities": caps,
                  "workspaceFolders": [{"uri": path_uri(self.root), "name": os.path.basename(self.root)}]}
        if self.init_options is not None:
            params["initializationOptions"] = self.init_options
        res = self.request("initialize", params, timeout=120)
        self.notify("initialized", {})
        return res

    def open_progress(self):
        with self.cv:
            return len(self.progress)

    def wait_ready(self, quiet=5.0, timeout=900):
        """Wait until no progress token is open and the server has been silent for `quiet` seconds."""
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            with self.cv:
                idle = not self.progress and time.monotonic() - self.last >= quiet
            if idle:
                return True
            time.sleep(min(0.1, quiet / 2 or 0.05))
        return False

    def open(self, rel, lang_id):
        path = os.path.join(self.root, rel)
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read()
        self.notify("textDocument/didOpen", {"textDocument": {"uri": path_uri(path), "languageId": lang_id, "version": 1, "text": text}})
        return path_uri(path)

    def close(self, rel):
        self.notify("textDocument/didClose", {"textDocument": {"uri": path_uri(os.path.join(self.root, rel))}})

    def shutdown(self):
        try:
            self.request("shutdown", None, timeout=20)
            self.notify("exit", None)
        except (RuntimeError, OSError):
            pass
        try:
            self.p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.p.kill()
