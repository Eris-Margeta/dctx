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
import select
import shutil
import subprocess
import tempfile
import time


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


_IDLE_SECONDS = 600
_CEILING_SECONDS = 3600
_POLL_SECONDS = 1
_LOCK_WARNING = "WARNING Cargo.lock changed after the gate left it; leaving concurrent bytes"


def _read_optional(path: Path):
    try:
        return path.read_bytes()
    except FileNotFoundError:
        return None


@contextmanager
def _preserve_lockfile(lockfile: Path):
    """Restore bytes this gate left unless a later writer replaced them."""
    original = _read_optional(lockfile)
    try:
        yield
    finally:
        if original is not None:
            left = _read_optional(lockfile)
            current = _read_optional(lockfile)
            if current != left:
                print(_LOCK_WARNING)
            elif current != original:
                lockfile.write_bytes(original)


def _parse_cpu_time(raw: str):
    text, days = " ".join(raw.split()), 0.0
    if not text:
        return None
    if "-" in text:
        day_text, text = text.split("-", 1)
        if not day_text.isdigit():
            return None
        days = float(day_text)
    try:
        parts = [float(part) for part in text.split(":")]
    except ValueError:
        return None
    if not 1 <= len(parts) <= 3:
        return None
    parts = [0.0] * (3 - len(parts)) + parts
    return ((days * 24 + parts[0]) * 60 + parts[1]) * 60 + parts[2]


def _process_cpu_seconds(pid: int):
    """Cumulative CPU seconds from `ps -o time=` with no shell."""
    try:
        completed = subprocess.run(
            ["ps", "-o", "time=", "-p", str(pid)], capture_output=True,
            text=True, check=False, shell=False, timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0:
        return None
    return _parse_cpu_time(completed.stdout)


def _read_pipes(proc):
    streams = [stream for stream in (proc.stdout, proc.stderr) if stream is not None]
    out, err = bytearray(), bytearray()
    while True:
        try:
            readable, _, _ = select.select(streams, [], [], 0) if streams else ([], [], [])
        except (OSError, ValueError):
            break
        chunks = []
        for stream in readable:
            try:
                chunks.append((stream, os.read(stream.fileno(), 65536)))
            except OSError:
                chunks.append((stream, b""))
        if not any(chunk for _stream, chunk in chunks):
            break
        for stream, chunk in chunks:
            (out if stream is proc.stdout else err).extend(chunk)
    return bytes(out), bytes(err)


class CargoLivenessExpired(subprocess.TimeoutExpired):
    """Killed because a named liveness rule fired."""

    def __init__(self, rule: str, elapsed: float):
        super().__init__("cargo", elapsed)
        self.rule = rule

    def __str__(self) -> str:
        return f"liveness rule {self.rule} fired after {self.timeout:g} seconds"


def _stop_child(proc, rule: str, elapsed: float):
    try:
        proc.kill()
    except OSError:
        pass
    try:
        proc.wait(timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        pass
    raise CargoLivenessExpired(rule, elapsed)


def _wait_lively(proc, read_output, cpu_seconds, monotonic, sleep):
    """Extend while output grows or CPU time advances; kill when a rule fires."""
    started = last_progress = monotonic()
    produced, last_cpu = 0, None
    stdout, stderr = bytearray(), bytearray()
    while True:
        out_chunk, err_chunk = read_output(proc)
        stdout += out_chunk or b""
        stderr += err_chunk or b""
        total = len(stdout) + len(stderr)
        try:
            cpu = cpu_seconds(proc.pid)
        except OSError:
            cpu = None
        now = monotonic()
        if total > produced or (last_cpu is not None and cpu is not None and cpu > last_cpu):
            last_progress = now
        produced = total
        if cpu is not None:
            last_cpu = cpu
        if proc.poll() is not None:
            out_chunk, err_chunk = read_output(proc)
            stdout += out_chunk or b""
            stderr += err_chunk or b""
            return proc.returncode, bytes(stdout), bytes(stderr)
        elapsed, idle = now - started, now - last_progress
        if elapsed >= _CEILING_SECONDS or idle >= _IDLE_SECONDS:
            rule = "hard-ceiling" if elapsed >= _CEILING_SECONDS else "no-progress"
            _stop_child(proc, rule, elapsed if rule == "hard-ceiling" else idle)
        sleep(_POLL_SECONDS)


def _run_cargo(command, workspace, env):
    proc = subprocess.Popen(
        list(command), cwd=str(workspace), env=env, shell=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, stdin=subprocess.DEVNULL,
    )
    for stream in (proc.stdout, proc.stderr):
        if stream is not None:
            os.set_blocking(stream.fileno(), False)
    code, out, err = _wait_lively(
        proc, _read_pipes, _process_cpu_seconds, time.monotonic, time.sleep,
    )
    return subprocess.CompletedProcess(
        list(command), code,
        out.decode("utf-8", "replace"), err.decode("utf-8", "replace"),
    )


def _cargo_tree(command: list[str], workspace: Path, env: dict[str, str]):
    """Try offline first; retry once only for an unavailable registry entry."""
    result = _run_cargo([*command, "--offline"], workspace, env)
    diagnostic = result.stderr.lower()
    missing_entry = (
        "no matching package named" in diagnostic
        or "failed to download" in diagnostic
        or ("failed to select a version" in diagnostic and "offline" in diagnostic)
    )
    if result.returncode and missing_entry:
        result = _run_cargo(command, workspace, {**env, "CARGO_NET_OFFLINE": "false"})
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
