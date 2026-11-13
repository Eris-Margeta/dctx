"""Report whether website inputs changed since the last successful deploy."""

import argparse
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--last-sha", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--paths-file", required=True, type=Path)
    args = parser.parse_args()

    if not args.last_sha:
        print("CHANGED")
        return

    try:
        prefixes = tuple(
            line.strip() for line in args.paths_file.read_text().splitlines()
            if line.strip()
        )
        paths = subprocess.check_output(
            ["git", "diff", "--name-only", "-z", "--end-of-options",
             f"{args.last_sha}..{args.head}", "--"],
            stderr=subprocess.DEVNULL,
        ).decode("utf-8", errors="surrogateescape").split("\0")
    except (OSError, subprocess.CalledProcessError):
        # Missing history must never hide a website update.
        print("CHANGED")
        return

    print("CHANGED" if any(path.startswith(prefixes) for path in paths) else "UNCHANGED")


if __name__ == "__main__":
    main()
