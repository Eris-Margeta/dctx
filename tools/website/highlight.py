"""Build-time syntax highlighting, written for this site: no library, no runtime script.

highlight(text, lang) returns escaped HTML with <span class="t-X"> tokens; lines(text, lang) returns the
same per line, with every span closed at the end of its line (terminal transcripts are one element per
line). The classes, coloured by the --hl-* tokens in site.css (check.py checks their contrast):

  t-k keyword     t-s string       t-c comment     t-m number, literal   t-f function, command, macro
  t-a key, flag   t-b type, kind   t-t tag, marker, section header       t-v variable, stable id   p the "$ " prompt (never copied)

Languages: the ones the site shows. sh (bash, zsh, console, shell), rust, python, js and ts, go, json,
toml (ini), yaml, gitignore, markdown, mermaid, dts (the dctx template source), and four dctx outputs:
pack (the snapshot format with its markers, each file body in its own language), map (records, ids and
the source a read returns), help (the CLI help text) and kv (key=value summary lines). "text" and an
unknown name detect a transcript ("$ " first) or JSON, and otherwise stay plain.
"""
from __future__ import annotations

import html
import json
import re
from typing import Callable

Tok = tuple[str, str]  # (class or "", text)

ALIASES = {"bash": "sh", "zsh": "sh", "shell": "sh", "console": "sh", "shell-session": "sh", "rs": "rust", "py": "python",
           "javascript": "js", "jsx": "js", "mjs": "js", "typescript": "js", "ts": "js", "tsx": "js", "golang": "go",
           "ini": "toml", "cfg": "toml", "yml": "yaml", "md": "markdown", "jsonc": "json", "dctx_template": "dts",
           "dctx-template": "dts", "mmd": "mermaid"}
EXT = {".rs": "rust", ".py": "python", ".pyi": "python", ".js": "js", ".mjs": "js", ".cjs": "js", ".ts": "js", ".tsx": "js",
       ".jsx": "js", ".go": "go", ".json": "json", ".toml": "toml", ".yaml": "yaml", ".yml": "yaml", ".md": "markdown",
       ".sh": "sh", ".gitignore": "gitignore", ".dctxignore": "gitignore", ".dctx_template": "dts", ".mmd": "mermaid"}


def lang_of_path(path: str) -> str:
    name = path.rsplit("/", 1)[-1]
    if name in (".gitignore", ".dctxignore"):
        return "gitignore"
    dot = name.rfind(".")
    return EXT.get(name[dot:].lower(), "") if dot > 0 else ""


# ------------------------------------------------------------------ the scanner
class Lexer:
    """Ordered (pattern, class) rules joined into one alternation. A class can be a function of the
    match and the previous non-space token, returning a class or a list of tokens."""

    def __init__(self, rules: list[tuple[str, str | Callable]], flags: int = re.M):
        self.rules = rules
        self.rx = re.compile("|".join(f"(?P<g{i}>{p})" for i, (p, _) in enumerate(rules)), flags)

    def __call__(self, text: str) -> list[Tok]:
        out: list[Tok] = []
        pos, prev = 0, ""
        for m in self.rx.finditer(text):
            if m.start() > pos:
                out.append(("", text[pos:m.start()]))
            kind = self.rules[int(m.lastgroup[1:])][1]
            got = kind(m, prev) if callable(kind) else kind
            if isinstance(got, list):
                out += got
            else:
                out.append((got, m.group(0)))
            if m.group(0).strip():
                prev = m.group(0)
            pos = m.end()
        if pos < len(text):
            out.append(("", text[pos:]))
        return out


def words(text: str) -> set[str]:
    return set(text.split())


def ident(keywords: set[str], types: set[str] = frozenset(), literals: set[str] = frozenset(),
          definers: set[str] = frozenset(), upper_type: bool = True) -> Callable:
    def classify(m: re.Match, prev: str) -> str:
        w = m.group(0)
        if w in literals:
            return "t-m"
        if w in keywords:
            return "t-k"
        if prev in definers:
            return "t-f"
        if w in types or (upper_type and w[:1].isupper() and not w.isupper()):
            return "t-b"
        rest = m.string[m.end():m.end() + 2]
        if rest.startswith("(") or rest.startswith("!(") or rest.startswith("::<"):
            return "t-f"
        return ""
    return classify


NUM = r"\b(?:0[xob][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)(?:[iuf](?:8|16|32|64|128|size)|n)?\b"
DQ = r'"(?:\\.|[^"\\\n])*"'
SQ = r"'(?:\\.|[^'\\\n])*'"

RUST = Lexer([
    (r"//[^\n]*", "t-c"), (r"/\*[\s\S]*?\*/", "t-c"), (r"#!?\[[^\]\n]*\]", "t-t"),
    (r'b?r(#*)"[\s\S]*?"\1', "t-s"), (r'b?"(?:\\.|[^"\\])*"', "t-s"), (r"b?'(?:\\.|[^'\\\n])'", "t-s"),
    (r"'[a-z_]\w*\b(?!')", "t-v"), (NUM, "t-m"), (r"[A-Za-z_]\w*!(?=[(\[{])", "t-f"),
    (r"[A-Za-z_]\w*", ident(words("as async await break const continue crate dyn else enum extern fn for if impl in let loop "
                                  "match mod move mut pub ref return self Self static struct super trait type unsafe use where while"),
                            words("bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 String Vec Option "
                                  "Result Box"), words("true false None Some Ok Err"), words("fn struct enum trait type mod"))),
])

PYTHON = Lexer([
    (r"#[^\n]*", "t-c"), (r'[rbuRBUfF]{0,2}"""[\s\S]*?"""', "t-s"), (r"[rbuRBUfF]{0,2}'''[\s\S]*?'''", "t-s"),
    (r"[rbuRBUfF]{0,2}" + DQ, "t-s"), (r"[rbuRBUfF]{0,2}" + SQ, "t-s"), (r"@[\w.]+", "t-t"), (NUM, "t-m"),
    (r"[A-Za-z_]\w*", ident(words("and as assert async await break class continue def del elif else except finally for from "
                                  "global if import in is lambda nonlocal not or pass raise return try while with yield"),
                            words("int str float bool bytes list dict set tuple object print len range open sum min max "
                                  "sorted enumerate zip isinstance super self cls"), words("True False None"),
                            words("def class"))),
])

JS = Lexer([
    (r"//[^\n]*", "t-c"), (r"/\*[\s\S]*?\*/", "t-c"), (r"`(?:\\.|[^`\\])*`", "t-s"), (DQ, "t-s"), (SQ, "t-s"), (NUM, "t-m"),
    (r"[A-Za-z_$][\w$]*", ident(words("abstract as async await break case catch class const continue default delete do else "
                                      "enum export extends finally for from function if implements import in instanceof "
                                      "interface let new of private protected public readonly return static switch this "
                                      "throw try type typeof var void while yield"),
                                words("string number boolean unknown any never void object Array Promise Map Set Record "
                                      "console JSON Math Object"), words("true false null undefined NaN"),
                                words("function class interface type enum"))),
])

GO = Lexer([
    (r"//[^\n]*", "t-c"), (r"/\*[\s\S]*?\*/", "t-c"), (r"`[^`]*`", "t-s"), (DQ, "t-s"), (r"'(?:\\.|[^'\\\n])'", "t-s"), (NUM, "t-m"),
    (r"[A-Za-z_]\w*", ident(words("break case chan const continue default defer else fallthrough for func go goto if import "
                                  "interface map package range return select struct switch type var"),
                            words("bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string "
                                  "uint uint8 uint16 uint32 uint64 uintptr any"), words("true false nil iota"),
                            words("func type"), upper_type=False)),
])

JSON = Lexer([
    (DQ + r"(?=\s*:)", "t-a"), (DQ, "t-s"), (r"-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b", "t-m"), (r"\b(?:true|false|null)\b", "t-k"),
])

TOML = Lexer([
    (r"(?:#|;)[^\n]*", "t-c"), (r"^[ \t]*\[\[?[^\]\n]+\]\]?", "t-t"), (r'^[ \t]*[\w.\-"]+(?=[ \t]*=)', "t-a"),
    (r'"""[\s\S]*?"""', "t-s"), (r"'''[\s\S]*?'''", "t-s"), (DQ, "t-s"), (SQ, "t-s"),
    (r"\b\d{4}-\d{2}-\d{2}(?:[T ][\d:.]+Z?)?\b", "t-m"), (NUM, "t-m"), (r"\b(?:true|false)\b", "t-k"),
])

YAML = Lexer([
    (r"#[^\n]*", "t-c"), (r"^[ \t]*(?:- )?[\w.\-/\"' ]+?(?=:(?:[ \t]|$))", "t-a"), (DQ, "t-s"), (SQ, "t-s"),
    (r"^[ \t]*-(?=[ \t])", "t-k"), (r"(?<=: )(?:true|false|null|yes|no)\b", "t-k"), (NUM, "t-m"), (r"[&*][\w-]+", "t-v"),
])

GITIGNORE = Lexer([(r"#[^\n]*", "t-c"), (r"^!", "t-k"), (r"\*\*?|/$", "t-m")])

MARKDOWN = Lexer([
    (r"^#{1,6} [^\n]*", "t-t"), (r"^```[^\n]*", "t-c"), (r"`[^`\n]+`", "t-s"), (r"^[ \t]*(?:[-*+]|\d+\.)(?=[ \t])", "t-k"),
    (r"\[[^\]\n]+\]\([^)\n]+\)", "t-a"), (r"\*\*[^*\n]+\*\*", "t-b"), (r"^ {4}[^\n]+", "t-s"), (r"^\|[-| :]+\|$", "t-c"),
])

MERMAID = Lexer([
    (r"%%[^\n]*", "t-c"), (r"\b(?:flowchart|graph|subgraph|end|classDef|class|style|LR|TD|TB|RL|BT)\b", "t-k"),
    (DQ, "t-s"), (r"-->|---|-\.->|==>|--", "t-t"), (r"\|[^|\n]*\|", "t-a"), (r"\b[a-z]\w*(?=\[|\(|\s*-)", "t-v"),
])

DTS_STR = Lexer([(r"\\.", "t-k"), (r"\{\{\{[^}]+\}\}\}", "t-v"), (r"\{\{[^}]+\}\}", "t-b")])


def _dts_string(m: re.Match, prev: str) -> list[Tok]:
    return [(c or "t-s", t) for c, t in DTS_STR(m.group(0))]


DTS = Lexer([
    (r"#[^\n]*", "t-c"), (r"^\[[A-Z_]+(?::[A-Z_]+)?\]", "t-t"), (r"\{\{\{[^}]+\}\}\}", "t-v"), (r"\{\{[^}]+\}\}", "t-b"),
    (r"^[a-z_]+(?=[ \t]*=)", "t-a"), (r'"(?:\\.|[^"\\])*"', _dts_string),
])


def _sh(text: str) -> list[Tok]:
    """Shell, one command line at a time: the prompt, the command word, flags, strings, variables, comments."""
    out: list[Tok] = []
    rx = re.compile(r"""(?P<sp>[ \t]+)|(?P<c>(?<![^\s])\#[^\n]*)|(?P<s>'[^']*'|"(?:\\.|[^"\\])*")|(?P<v>\$\{[^}]*\}|\$[\w@?#]+)"""
                    r"""|(?P<o>\|\||&&|[|;&<>]|\\$)|(?P<w>[^\s|;&<>'"$]+(?:[$'"][^\s|;&<>]*)?)""")
    keywords = words("if then else elif fi for while do done case esac in function export local return set unset alias source")
    for line in text.split("\n"):
        if out:
            out.append(("", "\n"))
        start = 0
        if line.startswith("$ "):
            out.append(("p", "$ "))
            start = 2
        cmd = True
        for m in rx.finditer(line, start):
            kind, tok = m.lastgroup, m.group(0)
            if kind == "sp":
                out.append(("", tok))
            elif kind == "c":
                out.append(("t-c", tok))
            elif kind == "s":
                out.append(("t-s", tok))
                cmd = False
            elif kind == "v":
                out.append(("t-v", tok))
                cmd = False
            elif kind == "o":
                out.append(("t-k", tok))
                cmd = tok != "\\"
            elif re.match(r"^\w+=", tok) and cmd:
                name, _, value = tok.partition("=")
                out += [("t-v", name), ("", "="), ("t-s" if value else "", value)]
            elif tok in keywords and cmd:
                out.append(("t-k", tok))
            elif cmd:
                out.append(("t-f", tok))
                cmd = False
            elif tok.startswith("-") and len(tok) > 1 and not tok[1].isdigit():
                flag, eq, value = tok.partition("=")
                out += [("t-a", flag + eq)] + ([("", value)] if value else [])
            else:
                out.append(("", tok))
    return out


def _help(text: str) -> list[Tok]:
    rx = Lexer([
        (r"^[A-Z][\w ]*:(?=[ \t]|$)", "t-t"), (r"\[(?:default|possible values|env|aliases): [^\]\n]*\]", "t-c"),
        (r"(?<![\w-])--?[A-Za-z][\w-]*", "t-a"), (r"<[A-Z][A-Z_]*>|\b[A-Z][A-Z_]{2,}\b", "t-v"), (r"`[^`\n]+`", "t-s"),
        (r"(?<=\s)dctx(?:-cli)?\b|^dctx\b", "t-f"), (r"(?<=[ =])\d+(?:\.\.\d+)?\b", "t-m"),
    ])
    return rx(text)


KV = Lexer([(r"\b[a-z_]+(?==)", "t-a"), (r"(?<==)[0-9a-f]{16,}\b", "t-v"), (r"(?<==)\d+\b", "t-m"), (r"(?<==)[A-Za-z]\w*", "t-s")])
MAP_ID = r"[smdkp]:[^\s\t]+"


def _map(text: str) -> list[Tok]:
    """dctx map and pack --for output: headers, records (edge, id, kind, path, lines) and the source a read returns."""
    out: list[Tok] = []
    lines = text.split("\n")
    read_all = bool(lines) and re.match(r"^# \S+ read$", lines[0]) is not None
    read_sec, code_lang, code_buf = False, "", []

    def flush() -> None:
        if code_buf:
            out.extend(tokens("\n".join(code_buf), code_lang))
            out.append(("", "\n"))
            code_buf.clear()

    for i, line in enumerate(lines):
        nl = ("", "\n") if i < len(lines) - 1 else None
        rec = re.match(r"^((?:calls|imports|contains)(?::(?:in|out))?:?\t?)?(" + MAP_ID + r")(\t[^\n]*)?$", line)
        if (read_all or read_sec) and code_lang and not rec and not line.startswith(("## ", "# end ")):
            code_buf.append(line)
            continue
        flush()
        if line.startswith("## "):
            verb, _, rest = line[3:].partition(" ")
            read_sec = verb == "read"
            code_lang = ""
            out += [("t-t", "## " + verb), ("", " " if rest else "")] + ([("t-v", rest)] if rest else [])
        elif line.startswith("#"):
            out += [("t-c", t) if not c else (c, t) for c, t in KV(line)]
        elif rec:
            edge, ident_, rest = rec.group(1) or "", rec.group(2), rec.group(3) or ""
            if edge:
                out.append(("t-c", edge))
            out.append(("t-v", ident_))
            fields = rest.split("\t")
            for j, f in enumerate(fields[1:], 1):
                out.append(("", "\t"))
                if j == 1:
                    out.append(("t-b", f))
                elif re.fullmatch(r"L\d+(?:-\d+)?|\d+", f):
                    out.append(("t-m", f))
                elif j == 3 and lang_of_path(ident_.split(":", 1)[-1].split("#")[0]) and not re.fullmatch(r"[\w./-]+", f):
                    out.extend(tokens(f, lang_of_path(ident_.split(":", 1)[-1].split("#")[0])))
                else:
                    out.append(("", f))
            if read_all or read_sec:
                code_lang = lang_of_path(ident_.split(":", 1)[-1].split("#")[0]) or "text"
        elif re.match(r"^(start|calls:(?:in|out)|imports:(?:in|out)|contains:(?:in|out)|gone)\t", line):
            head, _, rest = line.partition("\t")
            out += [("t-k", head), ("", "\t")] + _map(rest)
        elif m := re.match(r"^(  L)(\d+)(: )(.*)$", line):
            prev_path = next((t.split(":", 1)[-1].split("#")[0] for c, t in reversed(out) if c == "t-v"), "")
            out += [("t-m", "  L" + m.group(2)), ("", m.group(3))] + tokens(m.group(4), lang_of_path(prev_path))
        else:
            out += [("t-m" if re.fullmatch(r"\d+", w) else "", w) for w in re.split(r"(\d+)", line) if w]
        if nl:
            out.append(nl)
    if code_buf:
        out.extend(tokens("\n".join(code_buf), code_lang))
    return out


def _pack(text: str) -> list[Tok]:
    """The pack snapshot: header markers, instructions, the tree and each file body in its own language."""
    out: list[Tok] = []
    marker = Lexer([(r"</?[A-Z_]+", "t-t"), (r"(?<=\s)[A-Z_]+(?==)", "t-a"), (r'"[^"\n]*"', "t-s"), (r"/?>", "t-t")])
    body: list[str] = []
    lang = ""
    lines = text.split("\n")
    for i, line in enumerate(lines):
        start = re.match(r'^<FILE_CONTENT_START ID="[^"]*" PATH="([^"]*)"', line)
        if line.startswith("</FILE_CONTENT_END") or (body and i == len(lines) - 1 and line.startswith("…")):
            if body:
                out += _body(body, lang) + [("", "\n")]
                body = []
            lang = ""
        elif lang:
            body.append(line)
            continue
        if start:
            out += marker(line)
            lang = lang_of_path(start.group(1)) or "text"
        elif re.match(r"^\[(?:DCTX_[A-Z0-9_]+|DIRECTORY:[^\]]*)\]$", line):
            out.append(("t-k", line))
        elif line.startswith("<") and line.endswith(">"):
            out += marker(line)
        elif m := re.match(r"^(\s*)(\[[DF]\])(.*?)(\((?:ID|MOD|SIZE)[^)]*\))?$", line):
            out += [("", m.group(1)), ("t-k", m.group(2)), ("", m.group(3))] + ([("t-m", m.group(4))] if m.group(4) else [])
        elif line == "[Content omitted by pattern rules]" or line.startswith("… ("):
            out.append(("t-c", line))
        elif m := re.match(r"^(\s*)(\d+\.|-)( .*)$", line):
            out += [("", m.group(1)), ("t-k", m.group(2))] + marker(m.group(3))
        else:
            out.append(("", line))
        if i < len(lines) - 1:
            out.append(("", "\n"))
    if body:
        out += _body(body, lang)
    return out


def _body(body: list[str], lang: str) -> list[Tok]:
    text = "\n".join(body)
    return [("t-c", text)] if text == "[Content omitted by pattern rules]" else tokens(text, lang)


LEXERS: dict[str, Callable[[str], list[Tok]]] = {
    "rust": RUST, "python": PYTHON, "js": JS, "go": GO, "json": JSON, "toml": TOML, "yaml": YAML, "gitignore": GITIGNORE,
    "markdown": MARKDOWN, "mermaid": MERMAID, "dts": DTS, "sh": _sh, "help": _help, "map": _map, "pack": _pack, "kv": KV,
}


def detect(text: str) -> str:
    """The language of an unlabelled block: a shell transcript, a dctx output, JSON, or plain text."""
    head = text.lstrip()[:200]
    if head.startswith("$ "):
        return "transcript"
    if head.startswith("[DCTX_"):
        return "pack"
    if re.match(r"^# \S+ \w+\n|^# dctx pack", head):
        return "map"
    if head.startswith('{"') and text.rstrip().endswith("}"):
        return "json"
    if head[:1] in "{[":
        try:
            json.loads(text)
            return "json"
        except ValueError:
            try:
                for line in text.strip().split("\n"):
                    json.loads(line)
                return "json"
            except ValueError:
                pass
    return "text"


def output_lang(argv: list[str], stdout: str) -> str:
    """The language of a captured run's stdout, from the command and the text."""
    if "--help" in argv or "-h" in argv or argv[1:2] == ["help"]:
        return "help"
    found = detect(stdout)
    if found != "text":
        return found
    if "--markdown" in argv:
        return "markdown"
    if "mermaid" in argv:
        return "mermaid"
    if re.match(r"^\w+ \w+=", stdout):
        return "kv"
    return "text"


def _transcript(text: str) -> list[Tok]:
    """'$ command' lines in shell colours; the output after each in the language its text shows."""
    out: list[Tok] = []
    chunks = re.split(r"(?m)^(\$ [^\n]*)\n?", text)
    for k, chunk in enumerate(chunks):
        if not chunk:
            continue
        if k % 2:
            out += _sh(chunk) + [("", "\n")]
        else:
            argv = chunks[k - 1][2:].split() if k else []
            body = chunk[:-1] if chunk.endswith("\n") else chunk
            out += tokens(body, output_lang(argv, body) if argv else "text") + ([("", "\n")] if chunk.endswith("\n") else [])
    if out and out[-1] == ("", "\n") and not text.endswith("\n"):
        out.pop()
    return out


def tokens(text: str, lang: str) -> list[Tok]:
    lang = ALIASES.get(lang.lower(), lang.lower()) if lang else "text"
    if lang in ("text", "plain", "", "txt"):
        lang = detect(text)
    if lang == "transcript":
        return _transcript(text)
    fn = LEXERS.get(lang)
    return fn(text) if fn else [("", text)]


def lines(text: str, lang: str) -> list[str]:
    """Escaped HTML per line; a token that spans lines is closed and reopened at each line break."""
    rows: list[list[str]] = [[]]
    for cls, chunk in tokens(text, lang):
        for j, part in enumerate(chunk.split("\n")):
            if j:
                rows.append([])
            if part:
                rows[-1].append(f'<span class="{cls}">{html.escape(part, quote=False)}</span>' if cls else html.escape(part, quote=False))
    return ["".join(r) for r in rows]


def highlight(text: str, lang: str) -> str:
    return "\n".join(lines(text, lang))
