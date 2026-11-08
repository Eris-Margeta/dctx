# SPDX-License-Identifier: Apache-2.0
"""Project-scoped transport for dctx RPC v1. All output bytes come from the engine."""

from ._binary import BinaryNotFoundError, cache_binary, resolve_binary
from ._client import Client, EngineError, TransportError, VersionMismatchError
from ._generated import Hello, Verb
from ._version import __version__

__all__ = [
    "BinaryNotFoundError", "Client", "EngineError", "Hello", "TransportError",
    "Verb", "VersionMismatchError", "__version__", "cache_binary", "resolve_binary",
]
