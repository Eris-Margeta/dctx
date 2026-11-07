# SPDX-License-Identifier: Apache-2.0
"""Dependency-free PEP 517 wheel/sdist backend for this transport-only package."""

from __future__ import annotations

import base64
import csv
import gzip
import hashlib
import io
import tarfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def project() -> dict:
    return tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]


def metadata(name: str, version: str, description: str, *, dependencies: tuple[str, ...] = (), readme: str | None = None) -> bytes:
    lines = ["Metadata-Version: 2.4", f"Name: {name}", f"Version: {version}",
             f"Summary: {description}", "Requires-Python: >=3.11", "License-Expression: Apache-2.0",
             "License-File: LICENSE", "License-File: NOTICE",
             "Project-URL: Repository, https://github.com/Eris-Margeta/dctx"]
    if dependencies:
        lines.append("Provides-Extra: binary")
        for dependency in dependencies:
            package, marker = dependency.split(";", 1)
            lines.append(f'Requires-Dist: {package.strip()}; ({marker.strip()}) and extra == "binary"')
    lines += ["Description-Content-Type: text/markdown", "", readme or description, ""]
    return "\n".join(lines).encode("utf-8")


def write_wheel(output: str | Path, name: str, version: str, tags: tuple[str, ...],
                files: dict[str, tuple[bytes, int]], package_metadata: bytes, *, pure: bool) -> str:
    """Write a deterministic wheel with hashes and executable permission bits."""
    normalized = name.replace("-", "_")
    dist_info = f"{normalized}-{version}.dist-info"
    wheel_name = f"{normalized}-{version}-py3-none-{'.'.join(tags)}.whl"
    entries = dict(files)
    entries[f"{dist_info}/METADATA"] = (package_metadata, 0o644)
    wheel = ["Wheel-Version: 1.0", "Generator: dctx-python-build", f"Root-Is-Purelib: {str(pure).lower()}"]
    wheel += [f"Tag: py3-none-{tag}" for tag in tags]
    entries[f"{dist_info}/WHEEL"] = (("\n".join(wheel) + "\n").encode(), 0o644)
    for license_file in ("LICENSE", "NOTICE"):
        entries[f"{dist_info}/licenses/{license_file}"] = ((ROOT / license_file).read_bytes(), 0o644)
    record = io.StringIO(newline="")
    writer = csv.writer(record, lineterminator="\n")
    for path, (data, _) in sorted(entries.items()):
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode("ascii")
        writer.writerow((path, f"sha256={digest}", len(data)))
    record_path = f"{dist_info}/RECORD"
    writer.writerow((record_path, "", ""))
    entries[record_path] = (record.getvalue().encode("utf-8"), 0o644)
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output / wheel_name, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path, (data, mode) in sorted(entries.items()):
            info = zipfile.ZipInfo(path, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    return wheel_name


def build_wheel(wheel_directory: str, config_settings: dict | None = None,
                metadata_directory: str | None = None) -> str:
    config = project()
    version = config["version"]
    # This equality also works in an unpacked sdist without the repository.
    expected = f'__version__ = "{version}"'
    if expected not in (ROOT / "src/dctx/_version.py").read_text(encoding="utf-8").splitlines():
        raise ValueError("Python manifest and runtime versions differ")
    dependencies = tuple(config["optional-dependencies"]["binary"])
    if not all(requirement.split(";", 1)[0].strip().endswith(f"=={version}") for requirement in dependencies):
        raise ValueError("Every binary extra must pin the exact wrapper version")
    files = {}
    for path in sorted((ROOT / "src/dctx").iterdir()):
        if path.suffix == ".py" or path.name == "py.typed":
            files[f"dctx/{path.name}"] = (path.read_bytes(), 0o644)
    return write_wheel(wheel_directory, config["name"], version, ("any",), files,
                       metadata(config["name"], version, config["description"],
                                dependencies=dependencies,
                                readme=(ROOT / config["readme"]).read_text(encoding="utf-8")), pure=True)


def get_requires_for_build_wheel(config_settings: dict | None = None) -> list[str]:
    return []


def get_requires_for_build_sdist(config_settings: dict | None = None) -> list[str]:
    return []


def build_sdist(sdist_directory: str, config_settings: dict | None = None) -> str:
    config = project()
    stem = f"dctx-{config['version']}"
    paths = [ROOT / name for name in ("pyproject.toml", "build_backend.py", "README.md", "LICENSE", "NOTICE")]
    paths += [path for path in (ROOT / "src/dctx").iterdir() if path.suffix == ".py" or path.name == "py.typed"]
    output = Path(sdist_directory)
    output.mkdir(parents=True, exist_ok=True)
    filename = f"{stem}.tar.gz"
    with (output / filename).open("wb") as stream:
        with gzip.GzipFile(filename="", mode="wb", fileobj=stream, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for path in sorted(paths):
                    data = path.read_bytes()
                    info = tarfile.TarInfo(f"{stem}/{path.relative_to(ROOT).as_posix()}")
                    info.size, info.mode = len(data), 0o644
                    archive.addfile(info, io.BytesIO(data))
                info = tarfile.TarInfo(f"{stem}/PKG-INFO")
                data = metadata(config["name"], config["version"], config["description"],
                                dependencies=tuple(config["optional-dependencies"]["binary"]),
                                readme=(ROOT / config["readme"]).read_text(encoding="utf-8"))
                info.size, info.mode = len(data), 0o644
                archive.addfile(info, io.BytesIO(data))
    return filename
