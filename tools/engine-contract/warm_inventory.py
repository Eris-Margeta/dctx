#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Assert one directory-relative stat per file and one open per child directory.

Run from the repository root on Linux with strace and a release CLI binary.
The settled 4,000-file fixture and all state remain below target/.
"""
import collections
import os
from pathlib import Path
import re
import subprocess
import sys

from parity import isolated_env


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    base = Path("target/DCX-WARM-02/inventory-contract").resolve()
    repo = base / "project"
    for index in range(4000):
        path = repo / f"src/group{index // 100:02}/file{index:04}.rs"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"pub fn main_{index}() {{}}\n")
        os.utime(path, (1577836800, 1577836800))
    env = isolated_env(base / "state")
    env["DCTX_REUSE_REPORT"] = "1"
    command = [str(binary), "map", "find", "main", "--limit", "5"]
    subprocess.run(command, cwd=repo, env=env, capture_output=True, check=True)
    trace = base / "inventory.trace"
    result = subprocess.run(
        ["strace", "-f", "-yy", "-e", "trace=openat,newfstatat,getdents64",
         "-o", str(trace), *command],
        cwd=repo, env=env, capture_output=True, check=True,
    )
    report = result.stderr.decode()
    assert "generation reused\n" in report, report
    assert "cached_content_reads 0\n" in report, report
    assert "stamp_skipped_reads 4000\n" in report, report
    lines = trace.read_text()
    root = re.escape(str(repo))
    stats = re.findall(
        rf'newfstatat\(\d+<{root}/src/(group\d{{2}})>, "(file\d{{4}}\.rs)", [^\n]+AT_SYMLINK_NOFOLLOW\) = 0',
        lines,
    )
    assert len(stats) == 4000 and len(set(stats)) == 4000, len(stats)
    opens = re.findall(rf'openat\(\d+<{root}/src>, "(group\d{{2}})", ', lines)
    assert collections.Counter(opens) == {f"group{i:02}": 1 for i in range(40)}, opens
    # No rule-discovery probes or second directory stream may reopen a group.
    assert not re.search(rf'openat\(\d+<{root}/src/group\d{{2}}>, "\."', lines)
    assert not re.search(rf'openat\(\d+<{root}/src/group\d{{2}}>, "file\d{{4}}\.rs"', lines)
    print("PASS: 4000 unique fd-relative file stats; 40 child directories opened once; zero source file opens")
    return 0


if __name__ == "__main__":
    sys.exit(main())
