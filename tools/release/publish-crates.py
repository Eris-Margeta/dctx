#!/usr/bin/env python3
"""Compute the publication order without resolving unpublished registry packages."""
import argparse
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def publication_order():
    manifests = [*ROOT.glob("crates/*/Cargo.toml"), *ROOT.glob("plugins/*/Cargo.toml")]
    packages = {p.parent.resolve(): (p, tomllib.loads(p.read_text())) for p in manifests}
    public = {directory: item for directory, item in packages.items()
              if item[1]["package"].get("publish") is not False}
    pending, done, order = set(), set(), []

    def visit(directory):
        if directory in done:
            return
        if directory in pending:
            raise ValueError(f"publication dependency cycle at {directory}")
        if directory not in public:
            raise ValueError(f"public crate depends on private crate: {directory}")
        pending.add(directory)
        manifest, data = public[directory]
        tables = [data, *data.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies"):
                for dep in table.get(kind, {}).values():
                    if not isinstance(dep, dict) or "path" not in dep:
                        continue
                    dependency = (directory / dep["path"]).resolve()
                    version = packages[dependency][1]["package"]["version"]
                    if dep.get("version") != f"={version}":
                        raise ValueError(f"{manifest}: path dependency needs version ={version}")
                    visit(dependency)
        pending.remove(directory)
        done.add(directory)
        order.append(data["package"]["name"])

    for directory in sorted(public):
        visit(directory)
    return order


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true", help="run cargo publish --dry-run (no upload)")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--list", action="store_true", help="print order without invoking Cargo")
    args = parser.parse_args()
    order = publication_order()
    if args.list:
        print("\n".join(order))
        return
    for name in order:
        command = ["cargo", "publish", "--registry", "crates-io", "-p", name]
        for flag in ("dry_run", "offline", "allow_dirty"):
            if getattr(args, flag):
                command.append("--" + flag.replace("_", "-"))
        print("+ " + " ".join(command), flush=True)
        subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
