# SPDX-License-Identifier: Apache-2.0
"""Synchronous stdio transport. Engine output remains opaque."""

from __future__ import annotations

import json
import os
import re
import subprocess
import threading
from pathlib import Path
from typing import Any, cast

from ._binary import install_instructions, resolve_binary
from ._generated import Hello, Operations, Verb
from ._version import __version__


class TransportError(RuntimeError):
    """The subprocess or RPC envelope failed; no operation is retried."""


class VersionMismatchError(TransportError):
    """The engine release or RPC major version is incompatible."""


class EngineError(RuntimeError):
    """An unchanged engine failure, including exact stdout preceding failure."""

    def __init__(self, code: int, message: str, hint: str, output: bytes) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.hint = hint
        self.output = output


class Client(Operations):
    """One engine process rooted in an explicit project directory.

    Calls are serialized. close() drains the current call, then closes stdin.
    terminate() aborts running work from another thread. Diagnostics inherit
    stderr, so a verbose engine cannot block on an unread diagnostic pipe.
    """

    def __init__(self, project: str | os.PathLike[str], *,
                 binary: str | os.PathLike[str] | None = None) -> None:
        self.project = Path(project).expanduser().resolve(strict=True)
        if not self.project.is_dir():
            raise NotADirectoryError(self.project)
        self.binary = resolve_binary(binary)
        self._lock = threading.Lock()
        self._closed = False
        self._next_id = 0
        try:
            self._process = subprocess.Popen(
                [str(self.binary), "rpc"], cwd=self.project,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                # Inherit diagnostics, never buffer an unread stderr pipe.
                stderr=None, close_fds=True,
            )
        except OSError as exc:
            raise TransportError(f"Cannot start dctx engine: {exc}. {install_instructions()}") from exc
        try:
            self.hello()
        except BaseException:
            self.terminate()
            raise

    def _exchange(self, verb: Verb, args: tuple[str, ...]) -> dict[str, Any]:
        if self._closed:
            raise TransportError("dctx client is closed")
        if not all(isinstance(arg, str) for arg in args):
            raise TypeError("RPC arguments must be strings")
        request_id = self._next_id
        frame = json.dumps({"id": request_id, "verb": verb, "args": args},
                           ensure_ascii=True, separators=(",", ":")).encode("utf-8")
        if len(frame) > 1_048_576:
            raise ValueError("RPC request exceeds the engine's 1,048,576-byte frame limit")
        self._next_id += 1
        assert self._process.stdin is not None and self._process.stdout is not None
        try:
            self._process.stdin.write(frame + b"\n")
            self._process.stdin.flush()
            line = self._process.stdout.readline()
            if not line or not line.endswith(b"\n"):
                raise TransportError("dctx engine closed stdout before a complete response")
            reply = json.loads(line.decode("utf-8"))
            if not isinstance(reply, dict) or type(reply.get("id")) is not int or reply["id"] != request_id:
                raise TransportError("dctx response has an invalid correlation id")
            version = reply.get("engine_version")
            if version != __version__:
                raise VersionMismatchError(
                    f"dctx package {__version__} requires engine {__version__}; got {version!r}. "
                    f"Selected executable: {self.binary}. {install_instructions()}"
                )
            if ("result" in reply) == ("error" in reply):
                raise TransportError("dctx response must contain exactly one result or error")
            if "error" in reply:
                error = reply["error"]
                if (not isinstance(error, dict) or type(error.get("code")) is not int
                        or error["code"] not in (1, 2, 3)
                        or not all(isinstance(error.get(key), str) for key in ("message", "hint", "output"))):
                    raise TransportError("dctx response has an invalid error envelope")
                raise EngineError(error["code"], error["message"], error["hint"], error["output"].encode("utf-8"))
            if not isinstance(reply["result"], dict):
                raise TransportError("dctx response has an invalid result envelope")
            return reply["result"]
        except EngineError:
            raise
        except (OSError, UnicodeError, ValueError, TransportError) as exc:
            # A framing failure leaves correlation uncertain. End this session;
            # never retry an operation whose engine side effects are unknown.
            self._abort()
            if isinstance(exc, TransportError):
                raise
            raise TransportError(f"dctx transport failed: {exc}") from exc
        except BaseException:
            # An interrupted read may leave an operation in flight. It cannot be
            # safely correlated with a later call, so abort before propagating.
            self._abort()
            raise

    def hello(self) -> Hello:
        with self._lock:
            result = self._exchange("hello", ())
            protocol = result.get("protocol_version")
            verbs = result.get("verbs")
            if (not isinstance(protocol, str) or re.fullmatch(
                    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
                    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", protocol) is None):
                self._abort()
                raise TransportError("dctx hello has an invalid protocol version")
            if protocol.split(".")[0] != "1":
                self._abort()
                raise VersionMismatchError(f"dctx RPC major 1 required; got {protocol!r}")
            if not isinstance(verbs, list) or not all(isinstance(verb, str) for verb in verbs):
                self._abort()
                raise TransportError("dctx hello has an invalid verbs list")
            # Return only envelope metadata, never decode operation pages.
            return cast(Hello, result)

    def _abort(self) -> None:
        self._closed = True
        if self._process.poll() is None:
            self._process.kill()
        self._process.wait()

    def _operation(self, verb: Verb, args: tuple[str, ...]) -> bytes:
        with self._lock:
            result = self._exchange(verb, args)
            if not isinstance(result.get("output"), str):
                self._abort()
                raise TransportError("dctx result has no output string")
            try:
                return result["output"].encode("utf-8")
            except UnicodeError as exc:
                self._abort()
                raise TransportError("dctx output is not UTF-8") from exc

    def close(self, *, timeout: float = 5.0) -> None:
        """Graceful EOF after any active call; kill if shutdown exceeds timeout."""
        with self._lock:
            self._closed = True
            assert self._process.stdin is not None and self._process.stdout is not None
            try:
                self._process.stdin.close()
            except OSError:
                pass
            try:
                self._process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait()
            finally:
                self._process.stdout.close()

    def terminate(self) -> None:
        """Abort the process, including any active call, and close its pipes."""
        self._abort()
        self.close()

    def __enter__(self) -> Client:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()
