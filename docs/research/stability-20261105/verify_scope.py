#!/usr/bin/env python3
"""Check STAB-01's exact write scope, including the permitted status text spans."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
PIN = "0495f5f87717df5385209d306b92328a8cd96cf6"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT).decode()


assert git("rev-parse", "HEAD").strip() == PIN
assert subprocess.run(["git", "diff", "--cached", "--quiet"], cwd=ROOT).returncode == 0
tracked = set(git("diff", "--name-only").splitlines())
assert tracked == {"README.md", "docs/SPEC.md"}, tracked
new = git("ls-files", "--others", "--exclude-standard").splitlines()
assert "docs/cards/STAB-01-RESULT.md" in new
assert all(p == "docs/cards/STAB-01-RESULT.md" or p.startswith("docs/research/stability-20261105/") for p in new), new
assert not (ROOT / ".target-stab").exists()
for path in tracked:
    baseline = git("show", f"{PIN}:{path}")
    current = (ROOT / path).read_text()
    if path == "README.md":
        normalize = lambda text: re.sub(r"(?m)^- \*\*Byte-stable answers\*\*:.*$", "<STABILITY STATUS>", text)
    else:
        def normalize(text):
            text = re.sub(r"(?m)^\| \*\*PPE-3 Prefix-stable\*\*.*$", "<PPE-3 STATUS>", text)
            text = re.sub(r"(?m)^\| Text answers identical after an unrelated edit.*$", "<BASELINE STATUS>", text)
            text = re.sub(r"(?s)(`--json --provenance` )\(.*?TC-F03\);", r"\1<HEADER STATUS>;", text)
            return re.sub(r"(?s)(`read` prints the source span )\(.*?TC-F04\);", r"\1<READ STATUS>;", text)
    assert normalize(baseline) == normalize(current), path
print(f"PASS: {len(tracked)} tracked documentation files + {len(new)} new card/evidence files; only permitted status spans changed; no staged changes, engine changes, target artifacts or commits")
