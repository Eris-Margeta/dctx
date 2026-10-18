"""PP6 normal-dependency isolation gate for the PL-V runner.

Pure parsing and closure rules depend only on the Python standard library.
The CLI observes three separate Cargo resolutions; PL-V consumes verdicts.
This gate does not qualify binary contents or runtime process isolation.
"""

import argparse
from contextlib import contextmanager
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


FORBIDDEN = frozenset({
    "dctx-plugin-host", "dctx-plugin-platform", "dctx-plugin-git",
})
PROFILES = (
    ("facade-strict", "dctx", (
        "--no-default-features", "--features", "pack,map,templates,fs,segments,cache",
    ), True),
    ("cli-strict", "dctx-cli", (
        "--no-default-features", "--features", "strict",
    ), True),
    ("cli-default", "dctx-cli", (), False),
)
_TREE_PREFIX = re.compile(r"^(?:[│┃| ]*(?:[├└┣┗][─━]{2}|[|`]--)[ ]*)?")
_PACKAGE = re.compile(
    r"(?P<name>[A-Za-z0-9_][A-Za-z0-9_-]*) "
    r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"
    r"(?: \([^\r\n]*\)| \[[^\]\r\n]*\])*"
)


class InvalidTree(ValueError):
    """Cargo output is empty or cannot be completely interpreted."""


def parse_cargo_tree(output: str) -> set[str]:
    """Return crate names from prefix-none (or displayed tree) package lines.

    Versions, parenthesized source paths/duplicate markers and bracketed feature
    suffixes are metadata. Unknown nonempty lines refuse the entire observation.
    """
    names = set()
    for number, raw in enumerate(output.splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        line = _TREE_PREFIX.sub("", line, count=1)
        package = _PACKAGE.fullmatch(line)
        if package is None:
            raise InvalidTree(f"malformed package line {number}: {raw!r}")
        names.add(package.group("name"))
    if not names:
        raise InvalidTree("empty dependency closure")
    return names


def check_closure(output: str, root: str, strict: bool) -> set[str]:
    """Parse and enforce strict exclusion or the default Git-worker control."""
    names = parse_cargo_tree(output)
    if root not in names:
        raise InvalidTree(f"missing selected root {root}")
    if strict:
        leaked = {name for name in names
                  if name in FORBIDDEN or name.startswith("dctx-plugin-git")}
        if leaked:
            raise InvalidTree("forbidden dependencies: " + ", ".join(sorted(leaked)))
    elif "dctx-plugin-git" not in names:
        raise InvalidTree("positive control missing: dctx-plugin-git")
    return names


def _cargo_environment(scratch: Path) -> dict[str, str]:
    """Copy the offline registry cache so Cargo writes only under scratch."""
    env = os.environ.copy()
    original = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
    cargo_home = scratch / "cargo-home"
    cargo_home.mkdir()
    # Do not copy credentials, config, executable tools or other user files.
    registry = original / "registry"
    if registry.is_dir():
        shutil.copytree(registry, cargo_home / "registry")
    env.update({
        "CARGO_HOME": str(cargo_home),
        "CARGO_TARGET_DIR": str(scratch / "target"),
        "CARGO_NET_OFFLINE": "true",
        "RUSTUP_AUTO_INSTALL": "0",
        "TMPDIR": str(scratch),
        "TMP": str(scratch),
        "TEMP": str(scratch),
    })
    return env


def _one_line(message: str) -> str:
    return " ".join(message.split())


@contextmanager
def _preserve_lockfile(lockfile: Path):
    """Restore the original lockfile bytes even if resolution fails."""
    original = lockfile.read_bytes()
    try:
        yield
    finally:
        if not lockfile.exists() or lockfile.read_bytes() != original:
            lockfile.write_bytes(original)


def _cargo_tree(command: list[str], workspace: Path, env: dict[str, str]):
    """Try offline first; retry once only for an unavailable registry entry."""
    result = subprocess.run(
        [*command, "--offline"], cwd=workspace, env=env, capture_output=True,
        text=True, timeout=300, check=False,
    )
    diagnostic = result.stderr.lower()
    missing_entry = (
        "no matching package named" in diagnostic
        or "failed to download" in diagnostic
        or ("failed to select a version" in diagnostic and "offline" in diagnostic)
    )
    if result.returncode and missing_entry:
        result = subprocess.run(
            command, cwd=workspace, env={**env, "CARGO_NET_OFFLINE": "false"},
            capture_output=True, text=True, timeout=300, check=False,
        )
    return result


def run_gate(require_strict: bool = False) -> int:
    """Observe every profile independently; unavailable evidence is explicit."""
    cargo = shutil.which("cargo")
    if cargo is None:
        verdict = "FAIL" if require_strict else "SKIP"
        for label, _, _, _ in PROFILES:
            print(f"{verdict} {label}: cargo unavailable")
        return 1 if require_strict else 0

    workspace = Path(__file__).resolve().parents[2]
    failed = False
    with tempfile.TemporaryDirectory(prefix="dctx-isolation-") as directory:
        try:
            env = _cargo_environment(Path(directory))
        except OSError as error:
            for label, _, _, _ in PROFILES:
                print(f"FAIL {label}: cache preparation: {_one_line(str(error))}")
            return 1
        try:
            with _preserve_lockfile(workspace / "Cargo.lock"):
                for label, root, features, strict in PROFILES:
                    command = [cargo, "tree", "-p", root, *features, "-e", "normal",
                               "--prefix", "none", "--color", "never"]
                    try:
                        result = _cargo_tree(command, workspace, env)
                        if result.returncode:
                            detail = _one_line(result.stderr) or "no diagnostic"
                            raise InvalidTree(f"cargo exited {result.returncode}: {detail}")
                        names = check_closure(result.stdout, root, strict)
                    except (OSError, subprocess.TimeoutExpired, InvalidTree) as error:
                        failed = True
                        print(f"FAIL {label}: {_one_line(str(error))}")
                    else:
                        rule = "forbidden closure absent" if strict else "Git worker present"
                        print(f"PASS {label}: {len(names)} crates; {rule}")
        except OSError as error:
            for label, _, _, _ in PROFILES:
                print(f"FAIL {label}: lockfile preservation: {_one_line(str(error))}")
            return 1
    return int(failed)


def main() -> int:
    """Expose the required-evidence switch used by the kernel oracle."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--require-strict", action="store_true",
        help="fail when cargo is unavailable (invocation failures always fail)",
    )
    return run_gate(parser.parse_args().require_strict)


if __name__ == "__main__":
    raise SystemExit(main())
