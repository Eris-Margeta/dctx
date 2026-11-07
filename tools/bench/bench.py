#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Offline prompt-pair builder and whole-session cost calculator (stdlib only)."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import random
import re
import sys
from typing import Any

DEFAULT_TEMPLATE = (Path(__file__).resolve().parents[2]
                    / "crates/dctx-core/templates/default.dctx_template")
SCENARIOS = {"short", "long", "huge", "windows"}
PREFIX = "Answer the question using the source below. Cite source paths.\n\nQuestion:\n"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def finite(value: Any, label: str, minimum: float = 0,
           maximum: float = math.inf) -> float:
    if (isinstance(value, bool) or not isinstance(value, (int, float))
            or not math.isfinite(value) or not minimum <= value <= maximum):
        raise ValueError(f"{label} must be finite and in [{minimum}, {maximum}]")
    return value


def integer(value: Any, label: str, minimum: int = 0) -> int:
    finite(value, label, minimum)
    if not isinstance(value, int):
        raise ValueError(f"{label} must be an integer")
    return value


def counts(text: str, bytes_per_token: float) -> dict:
    size = len(text.encode("utf-8"))
    return {"utf8_bytes": size,
            "estimated_tokens": math.ceil(size / bytes_per_token),
            "sha256": digest(text.encode("utf-8"))}


def source_slice(root: Path, entry: dict, max_bytes: int) -> dict:
    """Read only named UTF-8 regular files; reject escapes and silent truncation."""
    name = entry["path"]
    path = PurePosixPath(name)
    if (not isinstance(name, str) or not name or path.is_absolute()
            or ".." in path.parts or "\\" in name or ":" in name
            or path.as_posix() != name or any(ord(c) < 32 for c in name)):
        raise ValueError(f"invalid root-relative POSIX path: {name!r}")
    source = (root / name).resolve(strict=True)
    if not source.is_relative_to(root) or not source.is_file():
        raise ValueError(f"source must be a regular file inside repo: {name}")
    with source.open("rb") as handle:
        raw = handle.read(max_bytes + 1)
    if len(raw) > max_bytes:
        raise ValueError(f"source exceeds --max-source-bytes: {name}")
    text = raw.decode("utf-8")  # Strict: no lossy repair and no CRLF translation.
    if "\x00" in text:
        raise ValueError(f"NUL in source: {name}")
    lines = text.splitlines(keepends=True)
    first = integer(entry.get("start_line", 1), "start_line", 1)
    last = integer(entry.get("end_line", len(lines)), "end_line")
    if ((not lines and (first != 1 or last != 0))
            or (lines and not 1 <= first <= last <= len(lines))):
        raise ValueError(f"invalid inclusive line range: {name}")
    selected = "".join(lines[first - 1:last])
    return {"path": name, "start_line": first, "end_line": last,
            "text": selected, "source_bytes": len(raw),
            "source_sha256": digest(raw),
            "selected_sha256": digest(selected.encode("utf-8"))}


def template_parts(raw: str) -> tuple[dict, dict]:
    """Read the bundled v1 template subset, not a general DTS compiler."""
    sections: dict[str, list[str]] = {}
    current = None
    for line in raw.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if re.fullmatch(r"\[[A-Z_:]+\]", line):
            current = line[1:-1]
            sections[current] = []
        elif current:
            sections[current].append(line)
    required = {"VARIABLES", "PARTIAL:TREE_ENTRY", "PARTIAL:FILE_BLOCK", "LAYOUT"}
    if set(sections) != required:
        raise ValueError("template must use the bundled v1 section subset")
    variables = {}
    for line in sections.pop("VARIABLES"):
        if line.strip():
            key, value = line.split("=", 1)
            # V1 is not JSON: it expands \n but preserves escaped quotes.
            variables[key.strip()] = value.strip().strip('"').replace("\\n", "\n")
    return variables, {key: "\n".join(value).strip()
                       for key, value in sections.items()}


def render_template(evidence: list[dict], raw_template: str) -> str:
    """Default v1 marker layout, with a constant project name and no CLI header.

    Sort children by relative path; assign directory/file IDs in preorder, as the
    v1 marker renderer does. All file content is inserted after variable expansion
    so literal template-like text in evidence remains untouched in both arms.
    """
    variables, parts = template_parts(raw_template)

    def expand(text: str, dynamic: dict | None = None) -> str:
        for _ in range(len(variables) + 1):
            updated = re.sub(r"(?<!\{)\{\{([\w]+)\}\}(?!\})",
                             lambda m: variables.get(m[1], m[0]), text)
            if updated == text:
                break
            text = updated
        if dynamic:
            text = re.sub(r"\{\{\{([\w.]+)\}\}\}",
                          lambda m: str(dynamic.get(m[1], m[0])), text)
        return text

    tree: dict = {}
    file_paths = {item["path"] for item in evidence}
    for item in evidence:
        node = tree
        components = item["path"].split("/")
        for part in components[:-1]:
            node = node.setdefault(part, {})
        node[components[-1]] = item
    tree_lines, blocks = [], []
    counter = 0

    def walk(node: dict, path: str, depth: int, is_file: bool = False) -> None:
        nonlocal counter
        if depth == 0:
            identity = "ROOT"
        else:
            counter += 1
            identity = f"{'F' if is_file else 'D'}{counter:03}"
        tree_lines.append("  " * depth + expand(parts["PARTIAL:TREE_ENTRY"], {
            "node.type_char": "[F]" if is_file else "[D]",
            "node.path": path, "node.id": identity}))
        if is_file:
            # File paths are also present verbatim in the plain arm.
            blocks.append(expand(parts["PARTIAL:FILE_BLOCK"], {
                "file.id": identity, "file.path": path,
                "file.lines": node["text"].count("\n") + 1,
                "file.content": node["text"]}))
        else:
            for name, child in sorted(node.items()):
                child_path = f"{path}/{name}" if path else name
                walk(child, child_path, depth + 1,
                     child_path in file_paths)

    walk(tree, "", 0)
    return expand(parts["LAYOUT"], {"project": "benchmark",
                  "comp.tree": "\n".join(tree_lines).strip(),
                  "comp.files": "\n".join(blocks).strip()})


def build_pairs(repo: Path, questions: dict, output: Path, models: list[str],
                runs: int = 5, seed: int = 0, bytes_per_token: float = 4,
                max_source_bytes: int = 16 * 1024 * 1024,
                template: Path = DEFAULT_TEMPLATE) -> dict:
    repo = repo.resolve(strict=True)
    if not repo.is_dir():
        raise ValueError("repo must be a directory")
    integer(runs, "runs", 3)
    integer(seed, "seed")
    finite(bytes_per_token, "bytes_per_token", 0.01)
    integer(max_source_bytes, "max_source_bytes", 1)
    if len(set(models)) < 2 or any(not model.strip() for model in models):
        raise ValueError("supply at least two distinct nonempty model labels")
    if len(set(models)) != len(models):
        raise ValueError("model labels must be unique")
    if questions.get("schema_version") != 1 or not questions.get("questions"):
        raise ValueError("expected schema_version 1 and nonempty questions")
    template_raw = template.read_bytes()
    template_text = template_raw.decode("utf-8")
    pairs, gold, prompts, ids = [], {}, {}, set()
    for question in questions["questions"]:
        identity = question["id"]
        if (not isinstance(identity, str)
                or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,79}", identity)
                or identity.casefold() in ids
                or re.fullmatch(r"CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]", identity, re.I)):
            raise ValueError("question IDs must be safe, case-insensitively unique names")
        ids.add(identity.casefold())
        if question["scenario"] not in SCENARIOS:
            raise ValueError(f"unknown scenario: {question['scenario']}")
        if not isinstance(question["question"], str) or not question["question"].strip():
            raise ValueError("question must be nonempty text")
        if not question.get("evidence") or not question.get("gold_facts"):
            raise ValueError("each question needs evidence and withheld gold_facts")
        facts = question["gold_facts"]
        if (any(not isinstance(f, dict) or not isinstance(f.get("id"), str)
                or not f["id"].strip() or not isinstance(f.get("fact"), str)
                or not f["fact"].strip() for f in facts) or len({f["id"] for f in facts}) != len(facts)):
            raise ValueError("gold_facts need unique IDs and nonempty fact text")
        evidence = [source_slice(repo, entry, max_source_bytes)
                    for entry in question["evidence"]]
        paths = [item["path"] for item in evidence]
        if len(set(paths)) != len(paths):
            raise ValueError("select each file once; combine spans before building")
        # Directory preorder matches the template, including a.txt/z before a/b.
        evidence.sort(key=lambda item: item["path"].split("/"))
        context = render_template(evidence, template_text)
        plain = "\n\n".join(f"{item['path']}\n{item['text']}" for item in evidence)
        shared = PREFIX + question["question"] + "\n\nSource:\n"
        a, b = shared + context, shared + plain
        target = max(len(a.encode("utf-8")), len(b.encode("utf-8")))
        unpadded_a, unpadded_b = counts(a, bytes_per_token), counts(b, bytes_per_token)
        padding_a, padding_b = target - len(a.encode()), target - len(b.encode())
        a += " " * padding_a
        b += " " * padding_b
        a_path, b_path = f"{identity}.dctx.txt", f"{identity}.plain.txt"
        prompts[a_path], prompts[b_path] = a, b
        pairs.append({"question_id": identity, "scenario": question["scenario"],
                      "evidence": [{k: v for k, v in item.items() if k != "text"}
                                   for item in evidence],
                      "dctx": {"file": a_path, **counts(a, bytes_per_token),
                               "padding_bytes": padding_a, "unpadded": unpadded_a},
                      "plain": {"file": b_path, **counts(b, bytes_per_token),
                                "padding_bytes": padding_b, "unpadded": unpadded_b}})
        gold[identity] = facts
    rng = random.Random(seed)
    schedule = []
    # Paired trials: new conversation for EACH arm; alternate order with a
    # randomized initial arm per model/question, and shuffle paired trial blocks.
    for model in models:
        for pair in pairs:
            first = rng.randrange(2)
            for run in range(runs):
                arms = ["dctx", "plain"] if (first + run) % 2 == 0 else ["plain", "dctx"]
                schedule.append({"model": model, "question_id": pair["question_id"],
                                 "run": run + 1, "arm_order": arms})
    rng.shuffle(schedule)
    manifest = {"schema_version": 1, "offline_only": True,
                "experiment": "template_ab_not_session_comparison",
                "renderer": "bundled-v1-marker-subset-no-cli-envelope",
                "template_sha256": digest(template_raw),
                "question_set_sha256": digest(json.dumps(questions, sort_keys=True,
                                                   ensure_ascii=False).encode()),
                "token_count": {"kind": "estimate_not_provider_usage",
                                "method": "ceil(utf8_bytes / bytes_per_token)",
                                "bytes_per_token": bytes_per_token},
                "length_match": "UTF-8 bytes; provider token equality NOT established",
                "seed": seed, "models": models, "runs_per_arm": runs,
                "pairs": pairs, "schedule": schedule}
    # Refuse reuse so artifacts cannot silently overwrite an earlier experiment.
    output.mkdir(parents=True, exist_ok=False)
    for name, prompt in prompts.items():
        (output / name).write_bytes(prompt.encode("utf-8"))
    write_json(output / "gold.json", gold)
    write_json(output / "pairs.json", manifest)
    return manifest


def cost_model(spec: dict, cached_discount: float = 0,
               input_price: float | None = None,
               output_price: float | None = None) -> dict:
    """Raw billed token totals stay unchanged by the cache price discount."""
    finite(cached_discount, "cached_discount", 0, 1)
    if (input_price is None) != (output_price is None):
        raise ValueError("provide both input and output prices, or neither")
    if input_price is not None:
        finite(input_price, "input_price")
        finite(output_price, "output_price")
    if spec.get("schema_version") != 1:
        raise ValueError("expected schema_version 1")
    rows = []
    if spec["kind"] == "projection":
        calls = integer(spec["calls"], "calls", 1)
        startup = integer(spec["startup_tokens"], "startup_tokens")
        context = integer(spec["context_tokens"], "context_tokens")
        growth = integer(spec.get("history_growth_tokens", 0), "history_growth_tokens")
        output = integer(spec.get("output_tokens_per_call", 0), "output_tokens_per_call")
        prefix = integer(spec.get("cached_prefix_tokens", 0), "cached_prefix_tokens")
        hit_share = finite(spec.get("cache_hit_share", 0), "cache_hit_share", 0, 1)
        if prefix > startup + context:
            raise ValueError("cached_prefix_tokens exceeds initial input")
        for index in range(calls):
            total = startup + context + index * growth
            cached = prefix * hit_share if index else 0  # First request is cold.
            rows.append({"input_tokens": total, "cached_input_tokens": cached,
                         "output_tokens": output})
    elif spec["kind"] == "usage":
        if not isinstance(spec["calls"], list) or not spec["calls"]:
            raise ValueError("usage calls must be a nonempty list")
        for call in spec["calls"]:
            total = integer(call["input_tokens"], "input_tokens")
            # Missing cache usage must not be silently reported as a miss.
            cached = integer(call["cached_input_tokens"], "cached_input_tokens")
            output = integer(call["output_tokens"], "output_tokens")
            if cached > total:
                raise ValueError("cached input cannot exceed total input")
            rows.append({"input_tokens": total, "cached_input_tokens": cached,
                         "output_tokens": output})
    else:
        raise ValueError("kind must be projection or usage")
    total_input = sum(row["input_tokens"] for row in rows)
    cached = sum(row["cached_input_tokens"] for row in rows)
    total_output = sum(row["output_tokens"] for row in rows)
    effective_input = total_input - cached_discount * cached
    return {"schema_version": 1, "kind": spec["kind"], "model_calls": len(rows),
            "input_tokens_including_cached": total_input,
            "cached_input_tokens": cached, "uncached_input_tokens": total_input - cached,
            "prompt_cache_hit_share": cached / total_input if total_input else None,
            "output_tokens_including_billed_reasoning": total_output,
            "total_billed_tokens": total_input + total_output,
            "cached_price_discount": cached_discount,
            "effective_input_tokens_at_uncached_rate": effective_input,
            "input_price_per_million": input_price,
            "output_price_per_million": output_price,
            "modeled_cost": ((effective_input * input_price + total_output * output_price)
                             / 1_000_000 if input_price is not None else None),
            "calls": rows}


def write_json(path: Path, value: Any) -> None:
    path.write_bytes((json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False)
                      + "\n").encode("utf-8"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build", help="build controlled template A/B pairs")
    build.add_argument("--repo", required=True, type=Path)
    build.add_argument("--questions", required=True, type=Path)
    build.add_argument("--out", required=True, type=Path)
    build.add_argument("--models", required=True, nargs="+")
    build.add_argument("--runs", type=int, default=5)
    build.add_argument("--seed", type=int, default=0)
    build.add_argument("--bytes-per-token", type=float, default=4)
    build.add_argument("--max-source-bytes", type=int, default=16 * 1024 * 1024)
    build.add_argument("--template", type=Path, default=DEFAULT_TEMPLATE)
    cost = commands.add_parser("cost", help="sum full usage or project repeated context")
    cost.add_argument("--spec", required=True, type=Path)
    cost.add_argument("--cached-discount", type=float, default=0,
                      help="price reduction: 0.9 means cached input costs 10%%")
    cost.add_argument("--input-price-per-million", type=float)
    cost.add_argument("--output-price-per-million", type=float)
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            result = build_pairs(args.repo, json.loads(args.questions.read_text(encoding="utf-8")),
                                 args.out, args.models, args.runs, args.seed,
                                 args.bytes_per_token, args.max_source_bytes, args.template)
            print(json.dumps({"pairs": len(result["pairs"]),
                              "paired_trials": len(result["schedule"]),
                              "out": str(args.out), "offline_only": True}))
        else:
            result = cost_model(json.loads(args.spec.read_text(encoding="utf-8")),
                                args.cached_discount, args.input_price_per_million,
                                args.output_price_per_million)
            print(json.dumps(result, indent=2, allow_nan=False))
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        parser.exit(2, f"bench: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
