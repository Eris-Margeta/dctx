# SPDX-License-Identifier: Apache-2.0
"""Local binary discovery only. This module never installs or downloads anything."""

from __future__ import annotations

import importlib.util
import os
import platform
import sys
from pathlib import Path

from ._version import __version__


class BinaryNotFoundError(FileNotFoundError):
    """No usable engine is installed at the selected location."""


def platform_key() -> str:
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    system = {"linux": "linux", "darwin": "darwin", "win32": "windows"}.get(
        sys.platform, sys.platform
    )
    return f"{system}_{arch}"


def cache_binary() -> Path:
    """The documented, versioned user cache location; never created by this package."""
    if sys.platform == "win32":
        root = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local")
    elif sys.platform == "darwin":
        root = Path.home() / "Library" / "Caches"
    else:
        root = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    return root / "dctx" / "bin" / __version__ / platform_key() / executable_name()


def executable_name() -> str:
    return "dctx.exe" if sys.platform == "win32" else "dctx"


def install_instructions() -> str:
    return (
        f'Install matching wheels explicitly: python -m pip install "dctx[binary]=={__version__}". '
        "For an offline wheelhouse: "
        f'python -m pip install --no-index --find-links ./wheelhouse "dctx[binary]=={__version__}". '
        "Until registry publication, use the CI wheel artifacts or build them with "
        "packages/python/tools/build_wheels.py (see packages/python/README.md). "
        f"Alternatively place the {__version__} engine at {cache_binary()}, "
        "or set DCTX_BIN to its absolute executable path. No automatic download is performed."
    )


def _require_executable(path: Path) -> Path:
    path = path.expanduser().resolve()
    if not path.is_file() or not os.access(path, os.X_OK):
        raise BinaryNotFoundError(f"Engine is missing or not executable: {path}. {install_instructions()}")
    return path


def resolve_binary(explicit: str | os.PathLike[str] | None = None) -> Path:
    """Resolve override, platform wheel, then cache. Never search PATH or fall back on failure.

    An explicit path overrides DCTX_BIN; otherwise DCTX_BIN is authoritative.
    Relative override paths resolve in the caller's working directory.
    """
    override = explicit if explicit is not None else os.environ.get("DCTX_BIN")
    if override is not None:
        return _require_executable(Path(override))
    module = f"dctx_binary_{platform_key()}"
    spec = importlib.util.find_spec(module)
    if spec is not None:
        if spec.origin is None:
            raise BinaryNotFoundError(f"Installed platform package {module} has no file location. {install_instructions()}")
        return _require_executable(Path(spec.origin).parent / executable_name())
    candidate = cache_binary()
    if candidate.is_file():
        return _require_executable(candidate)
    raise BinaryNotFoundError(f"No dctx {__version__} engine for {platform_key()}. {install_instructions()}")
