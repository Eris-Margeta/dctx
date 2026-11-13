#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check documented dctx verbs/flags using only the selected binary's help.

Scans README.md, docs/*.md and website/**/*.md or *.html from the repository
root (the current directory). Markdown examples must be fenced; HTML examples
must be in <pre> blocks. Fences labelled text or legacy, and blocks immediately
preceded by a paragraph containing the word legacy, are excluded. Inline prose
and command output are not shell examples. No documented command is executed.
"""

import argparse
from dataclasses import dataclass, field
from html.parser import HTMLParser
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys


LEGACY = re.compile(r"\blegacy\b", re.I)
FLAG = re.compile(r"(?<![\w-])(--[a-zA-Z][\w-]*|-[a-zA-Z])\b")
COMMAND = re.compile(r"^\s*(?:\$\s+|(?:Usage|Example):\s*)?dctx\s+(.+)$")
HELP_COMMAND = re.compile(r"^\s*(?:Usage:\s*)?dctx\s+(.+)$")
FENCE = re.compile(r"^\s*(?:>\s*)?(`{3,}|~{3,})(.*)$")


class CodeBlocks(HTMLParser):
    """Collect preformatted text, retaining source lines through nested spans."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.blocks = []
        self.current = None
        self.skip = False
        self.previous = ""
        self.paragraph = ""
        self.spans = []

    def handle_starttag(self, tag, attrs):
        if self.current is None and tag in {"p", "h1", "h2", "h3", "h4", "h5", "h6", "li"}:
            self.paragraph = ""
        if tag == "pre":
            self.current = []
            self.skip = bool(LEGACY.search(self.previous))
            self.spans = []
        if self.current is not None and tag in {"pre", "code"}:
            labels = " ".join(value or "" for _, value in attrs)
            self.skip |= bool(re.search(r"\b(?:language-|lang-)?text\b", labels, re.I)
                              or LEGACY.search(labels))
        if self.current is not None and tag == "br":
            self.current.append((self.getpos()[0], "\n"))
        if self.current is not None and tag == "span":
            classes = dict(attrs).get("class", "").split()
            self.spans.append(classes)
            if "ln" in classes:
                self.current.append((self.getpos()[0], "\n"))

    def handle_endtag(self, tag):
        if self.current is not None and tag == "span" and self.spans:
            if "ln" in self.spans.pop():
                self.current.append((self.getpos()[0], "\n"))
        if tag == "pre" and self.current is not None:
            if not self.skip:
                self.blocks.append(self.current)
            self.current = None
            self.previous = ""
            self.paragraph = ""

    def handle_data(self, data):
        if self.current is None:
            if data.strip():
                self.paragraph += data
                self.previous = self.paragraph.strip()
        elif not any("ln" in classes and ("out" in classes or "meta" in classes)
                     for classes in self.spans):
            self.current.append((self.getpos()[0], data))


def html_lines(chunks):
    line, text = None, ""
    for start, data in chunks:
        for offset, piece in enumerate(data.splitlines(keepends=True)):
            if line is None:
                line = start + offset
            text += piece.rstrip("\r\n")
            if piece.endswith(("\r", "\n")):
                yield line, text
                line, text = None, ""
    if line is not None:
        yield line, text


def blocks(source, html=False):
    if html:
        parser = CodeBlocks()
        parser.feed(source)
        for chunks in parser.blocks:
            yield list(html_lines(chunks))
        return
    marker, skip, previous, current = None, False, "", []
    paragraph_break = True
    for number, line in enumerate(source.splitlines(), 1):
        fence = FENCE.match(line)
        if marker is None:
            if fence:
                marker = fence[1]
                info = fence[2].strip().lower().split()
                skip = "text" in info or bool(LEGACY.search(fence[2] + " " + previous))
                current = []
                previous = ""
                paragraph_break = True
            elif line.strip():
                previous = line if paragraph_break else previous + " " + line
                paragraph_break = False
            else:
                paragraph_break = True
        elif fence and fence[1][0] == marker[0] and len(fence[1]) >= len(marker) and not fence[2].strip():
            if not skip:
                yield current
            marker = None
        else:
            current.append((number, line))


def commands(lines):
    """Join shell continuations without losing the offending flag's line."""
    pending, locations, legacy = "", [], False
    for number, line in lines:
        if not pending:
            match = COMMAND.match(line)
            if not match or match[1].lstrip().startswith("="):
                if line.strip():
                    legacy = bool(LEGACY.search(line))
                continue
            # TUI banners and version output are not shell command lines.
            if re.match(r"(?:-\s+\S|\d+\.\d+\.\d+\b)", match[1]):
                continue
            if legacy:
                legacy = False
                continue
            line = match[1]
        continuation = line.rstrip().endswith("\\")
        piece = line.rstrip()[:-1] if continuation else line
        locations.extend([number] * len(piece))
        pending += piece
        if continuation:
            pending += " "
            locations.append(number)
            continue
        yield pending, locations
        pending, locations = "", []
    if pending:
        yield pending, locations


def flags(text):
    result = {}
    for match in FLAG.finditer(text):
        tail = text[match.end():]
        takes_value = bool(re.match(
            r"[ \t](?![ \t])(?:<[^>]+>|[A-Z][A-Z_]*\b|[a-z]+(?:\|[\w-]+)+|\d+\.\.\d+)", tail))
        result[match[0]] = result.get(match[0], False) or takes_value
    return result


def usage_paths(syntax):
    """Expand literal verbs and <verb|verb> choices, stopping at arguments."""
    paths = [()]
    for word in syntax.split():
        word = word.strip(",")
        if word.startswith("<") and "|" not in word:
            break
        choice = word.strip("<>")
        if not re.fullmatch(r"[a-z][a-z0-9-]*(?:\|[a-z][a-z0-9-]*)*", choice):
            break
        paths = [path + (part,) for path in paths for part in choice.split("|")]
    return paths


@dataclass
class HelpNode:
    children: set = field(default_factory=set)
    options: dict = field(default_factory=dict)


class Help:
    def __init__(self, binary):
        self.binary = str(binary.resolve())
        self.nodes = {(): HelpNode()}
        self.loaded = set()
        self.unavailable = set()
        self.load(())
        if not self.nodes[()].children:
            raise ValueError("binary help contains no recognizable commands")

    def node(self, path):
        for length in range(len(path)):
            self.nodes.setdefault(path[:length], HelpNode()).children.add(path[length])
        return self.nodes.setdefault(path, HelpNode())

    def load(self, path):
        if path in self.loaded:
            return
        self.loaded.add(path)
        if any(path[:length] in self.unavailable for length in range(1, len(path))):
            return
        process = subprocess.run([self.binary, *path, "--help"], capture_output=True,
                                 text=True, timeout=15, check=False,
                                 env={**os.environ, "DCTX_AUTO_CLEAN": "0"})
        page = process.stdout + "\n" + process.stderr
        if process.returncode:
            if not path:
                raise ValueError(f"dctx --help exited {process.returncode}")
            # Some custom CLI groups expose their shapes only on a parent page.
            # Keep that authoritative synopsis when a descendant has no help.
            self.unavailable.add(path)
            return
        listing = False
        usage = False
        for line in page.splitlines():
            stripped = line.strip()
            if stripped.lower().startswith("usage:"):
                usage = True
            elif not stripped:
                usage = False
            match = HELP_COMMAND.match(line) if usage else None
            if match:
                for command in usage_paths(match[1]):
                    self.node(command).options.update(flags(match[1]))
            if re.fullmatch(r"(?:Subcommands|Commands):", stripped, re.I):
                listing = True
            elif listing and stripped:
                entry = re.match(r"^\s{2,}([a-z][a-z0-9-]*)\s{2,}", line)
                if entry:
                    self.node(path + (entry[1],))
                else:
                    listing = False
            # Option declarations apply to this page, not every synopsis on it.
            if re.match(r"^\s+-[a-zA-Z-]", line) or stripped.startswith("`--"):
                self.node(path).options.update(flags(line))

    def check(self, text):
        try:
            words = shlex.split(text, comments=True)
        except ValueError as error:
            return [("syntax", str(error))]
        path, options, errors = (), {}, []
        verb_failed, positional, after_dash = False, False, False
        index = 0
        while index < len(words):
            self.load(path)
            node = self.nodes[path]
            options.update(node.options)
            raw = words[index]
            index += 1
            if raw in {"|", "||", "&&", ";", ">", ">>", "2>", "2>&1"}:
                break
            word = raw.strip("[]")
            if after_dash:
                continue
            if word == "--":
                after_dash = True
                continue
            if word.startswith("-") and word != "-":
                candidates = word.split("=", 1)[0].split("|")
                for option in candidates:
                    if option not in options:
                        errors.append(("flag", option))
                    elif options[option] and "=" not in word and len(candidates) == 1:
                        index += 1
                continue
            if not positional and node.children and not verb_failed:
                choices = word.strip("<>").split("|")
                unknown = [choice for choice in choices if choice not in node.children]
                if unknown and not (word.startswith("<") and len(choices) == 1):
                    errors.extend(("verb", choice) for choice in unknown)
                    verb_failed = True
                elif len(choices) == 1 and not unknown:
                    path += (word,)
                else:
                    # Synopsis alternatives each have their own flag surface.
                    rest = " ".join(words[index:])
                    for choice in choices:
                        if choice in node.children:
                            errors.extend(self.check(" ".join((*path, choice, rest))))
                    break
            else:
                positional = True
        return list(dict.fromkeys(errors))


def sources(root):
    paths = [root / "README.md", *sorted((root / "docs").glob("*.md"))]
    paths.extend(sorted(path for path in (root / "website").rglob("*")
                        if path.suffix.lower() in {".md", ".markdown", ".html", ".htm"}))
    return [path for path in paths if path.is_file()]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="dctx binary whose help is authoritative")
    args = parser.parse_args(argv)
    root = Path.cwd()
    binary = args.binary
    if binary is None:
        binary = next((root / name for name in ("target/debug/dctx", "target/release/dctx")
                       if (root / name).is_file()), None)
        if binary is None:
            print("NOTE: no target/debug/dctx or target/release/dctx; build dctx or pass --binary PATH")
            return 0
    try:
        help_pages = Help(binary)
        count, drift = 0, 0
        for path in sources(root):
            for block in blocks(path.read_text(encoding="utf-8"), path.suffix.lower() in {".html", ".htm"}):
                for command, locations in commands(block):
                    count += 1
                    for kind, value in help_pages.check(command):
                        offset = command.find(value)
                        number = locations[max(0, offset)] if locations else block[0][0]
                        print(f"{path.relative_to(root)}:{number}: unknown {kind}: {value}")
                        drift += 1
        print(f"Checked {count} commands; {drift} drift issue(s).")
        return int(bool(drift))
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
