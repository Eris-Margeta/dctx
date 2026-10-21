#!/usr/bin/env python3
"""Independent, tool-agnostic definition and call-site scanner (Python 3 stdlib).

This is the adjudication oracle for the call-graph accuracy measurement. It is
line-based and deliberately conservative: comments and string literals are
blanked first, then a line is a call site of NAME only if NAME is directly
followed by an argument list. Definition lines are excluded. Collisions are
resolved by simple scope rules (same file, same package, explicit import,
`self`/`this`, `Type::`); anything else is marked ambiguous and excluded from
precision and recall rather than guessed.
"""
import os
import re
import subprocess
from collections import defaultdict

LANG_EXT = {
    ".py": "py", ".pyi": "py",
    ".rs": "rs",
    ".go": "go",
    ".ts": "ts", ".tsx": "ts", ".mts": "ts", ".cts": "ts",
    ".js": "ts", ".jsx": "ts", ".mjs": "ts", ".cjs": "ts",
}
SKIP_DIRS = ("graft/", ".trail/", ".dctx/", "node_modules/", "target/", "vendor/")
KEYWORDS = set("""if for while switch catch return function constructor super new typeof await yield
match loop else elif with assert print lambda def class fn impl func go defer select case struct enum
type interface import export from as in is not and or del pass raise try except finally global nonlocal
self this Self None True False nil null undefined""".split())


# Method names that also exist on standard-library / builtin types in the four languages. A
# receiver call `x.<name>(` with one of these names cannot be bound by name alone.
STD_METHODS = set("""get set setdefault update pop append extend insert remove clear copy keys values items
join split strip format read write close open send flush run start stop next iter len push map filter reduce
then catch finally resolve reject apply call bind toString valueOf parse stringify encode decode lower upper
replace find index count sort reverse add discard union clone into from default fmt build finish unwrap expect
as_ref as_mut to_string to_owned borrow lock with_capacity contains is_empty iter_mut collect extend_from_slice
write_all read_to_string read_line Error String Write Read Close Len Less Swap ServeHTTP Get Set Add Delete Lock
Unlock Done Wait Do Err Value Next Scan Exec Query Load Store Range forEach some every includes indexOf slice splice
emit on once off has entries create load save delete handle process execute dispatch render validate serialize
deserialize first last and_then or_else map_err ok_or ok_or_else unwrap_or unwrap_or_else unwrap_or_default
is_some is_none is_ok is_err take as_str as_bytes as_slice as_deref chars bytes lines trim trim_start trim_end
starts_with ends_with split_once to_vec to_lowercase to_uppercase get_mut entry or_insert or_insert_with retain drain
truncate sort_by sort_by_key dedup windows chunks zip enumerate rev skip take_while filter_map flat_map fold any all
position max min sum product cloned copied peekable chain push_str capacity reserve error warn info debug trace kind
source description display path file_name extension parent exists is_file is_dir metadata canonicalize read_dir
create_dir_all remove_file Bytes Reset Context WithValue Name Type Kind Field Header Del Path Scheme Host WriteHeader
Encode Decode Marshal Unmarshal Hash Sum New Now Format Parse Unix Sub Before After Equal IsZero Run Start Stop
shift unshift findIndex concat keys toLowerCase toUpperCase startsWith endsWith match test exec log json text end
warning exception lstrip rstrip""".split())


def lang_of(path):
    return LANG_EXT.get(os.path.splitext(path)[1].lower())


def list_files(root):
    p = subprocess.run(["git", "-C", root, "-c", "safe.directory=*", "ls-files", "-z"], capture_output=True)
    files = [f for f in p.stdout.decode("utf-8", "surrogateescape").split("\0") if f]
    return [f for f in files if lang_of(f) and not f.startswith(SKIP_DIRS)
            and os.path.isfile(os.path.join(root, f)) and not os.path.islink(os.path.join(root, f))]


# ---------------------------------------------------------------- stripping
_STR_PATTERNS = {
    "py": re.compile(r'''[rbfuRBFU]{0,2}("([^"\\\n]|\\.)*"|'([^'\\\n]|\\.)*')'''),
    "rs": re.compile(r'''b?"([^"\\]|\\.)*"|'([^'\\\n]|\\.){1,4}'(?!\w)'''),
    "go": re.compile(r'''"([^"\\\n]|\\.)*"|`[^`]*`|'([^'\\\n]|\\.)+\''''),
    "ts": re.compile(r'''"([^"\\\n]|\\.)*"|'([^'\\\n]|\\.)*'|`([^`\\]|\\.)*`'''),
}


def blank(m):
    return " " * len(m.group(0))


def _clean_c_like(text, lang):
    """Blank comments and string/char literals for Rust, Go and JS/TS with a state machine that
    carries state across lines (multi-line strings, raw strings, block comments)."""
    out = []
    i, n = 0, len(text)
    state, close = "code", None
    ident = lambda ch: ch.isalnum() or ch == "_"
    while i < n:
        c = text[i]
        if c == "\n":
            out.append("\n")
            if state == "line":
                state = "code"
            i += 1
            continue
        if state == "code":
            two = text[i:i + 2]
            if two == "//":
                state = "line"
                out.append("  ")
                i += 2
                continue
            if two == "/*":
                state = "block"
                out.append("  ")
                i += 2
                continue
            prev = text[i - 1] if i else " "
            if lang == "rs" and c in "rb" and not ident(prev):
                j = i + 1 if c == "r" else (i + 2 if text[i + 1:i + 2] == "r" else -1)
                if j > 0:
                    k = j
                    while k < n and text[k] == "#":
                        k += 1
                    if k < n and text[k] == '"':
                        state, close = "raw", '"' + "#" * (k - j)
                        out.append(" " * (k + 1 - i))
                        i = k + 1
                        continue
            if c == '"' or (c == "`" and lang in ("ts", "go")) or (c == "'" and lang == "ts"):
                state, close = "str", c
                out.append(" ")
                i += 1
                continue
            if c == "'" and lang in ("rs", "go"):
                if text[i + 1:i + 2] == "\\":
                    k = text.find("'", i + 2)
                    if 0 < k - i <= 12:
                        out.append(" " * (k + 1 - i))
                        i = k + 1
                        continue
                elif text[i + 2:i + 3] == "'":
                    out.append("   ")
                    i += 3
                    continue
            out.append(c)
            i += 1
            continue
        if state == "line":
            out.append(" ")
            i += 1
            continue
        if state == "block":
            if text[i:i + 2] == "*/":
                state = "code"
                out.append("  ")
                i += 2
            else:
                out.append(" ")
                i += 1
            continue
        if state == "raw":
            if text.startswith(close, i):
                state = "code"
                out.append(" " * len(close))
                i += len(close)
            else:
                out.append(" ")
                i += 1
            continue
        # quoted string
        if c == "\\" and not (lang == "go" and close == "`"):
            out.append("  " if i + 1 < n and text[i + 1] != "\n" else " ")
            i += 2 if i + 1 < n and text[i + 1] != "\n" else 1
            continue
        if c == close:
            state = "code"
        out.append(" ")
        i += 1
    return "".join(out).split("\n")


def _clean_ts_per_line(text):
    """JS/TS: per-line blanking (regex literals make a cross-line string state machine unreliable);
    block comments are tracked across lines."""
    out, in_block = [], False
    for line in text.split("\n"):
        res, i = "", 0
        while i < len(line):
            if in_block:
                j = line.find("*/", i)
                if j < 0:
                    res += " " * (len(line) - i)
                    break
                res += " " * (j + 2 - i)
                i, in_block = j + 2, False
                continue
            rest = _STR_PATTERNS["ts"].sub(blank, line[i:])
            c1, c2 = rest.find("//"), rest.find("/*")
            cut = [c for c in (c1, c2) if c >= 0]
            if not cut:
                res += rest
                break
            k = min(cut)
            res += rest[:k]
            if k == c1:
                res += " " * (len(rest) - k)
                break
            res += "  "
            i += k + 2
            in_block = True
        out.append(res)
    return out


def clean_lines(text, lang):
    """Return lines with strings and comments blanked (same length, same line count)."""
    text = text.replace("\r", "")
    if lang in ("rs", "go"):
        return _clean_c_like(text, lang)
    if lang == "ts":
        return _clean_ts_per_line(text)
    lines = text.split("\n")
    out = []
    tq = None
    for line in lines:
        res = ""
        i = 0
        while i < len(line):
            if tq:
                j = line.find(tq, i)
                if j < 0:
                    res += " " * (len(line) - i)
                    i = len(line)
                else:
                    res += " " * (j + 3 - i)
                    i = j + 3
                    tq = None
                continue
            m = re.search(r'("""|\'\'\')', line[i:])
            seg_end = i + m.start() if m else len(line)
            seg = _STR_PATTERNS["py"].sub(blank, line[i:seg_end])
            h = seg.find("#")
            if h >= 0:
                res += seg[:h] + " " * (len(line) - i - h)
                i = len(line)
                break
            res += seg
            i = seg_end
            if m:
                tq = m.group(1)
                res += "   "
                i += 3
        out.append(res)
    return out


# ---------------------------------------------------------------- definitions
PY_DEF = re.compile(r"^(\s*)(?:async\s+)?def\s+(\w+)\s*[\[(]")
PY_CLASS = re.compile(r"^(\s*)class\s+(\w+)\s*[(:\[]?(.*)")
RS_FN = re.compile(r"\bfn\s+(\w+)\s*[<(]")
RS_IMPL_FOR = re.compile(r"^\s*(?:unsafe\s+)?impl\b(?:\s*<[^{]*?>)?\s+([\w:<>, &']+?)\s+for\s+([\w:]+)")
RS_IMPL = re.compile(r"^\s*(?:unsafe\s+)?impl\b(?:\s*<[^{]*?>)?\s+&?(?:'\w+\s+)?([\w:]+)")
RS_TRAIT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+(\w+)")
GO_METHOD = re.compile(r"^func\s*\(\s*\w*\s*\*?\s*(\w+)(?:\[[^\]]*\])?\s*\)\s*(\w+)\s*[\[(]")
GO_FUNC = re.compile(r"^func\s+(\w+)\s*[\[(]")
GO_IFACE = re.compile(r"^type\s+(\w+)(?:\[[^\]]*\])?\s+interface\s*\{")
GO_IFACE_M = re.compile(r"^\s+(\w+)\s*\(")
TS_FUNC = re.compile(r"^\s*(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:async\s+)?function\s*\*?\s*(\w+)")
TS_ARROW = re.compile(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*(?::[^=]+)?=\s*(?:async\s+)?(?:function\b|\([^)]*\)\s*(?::[^=]+)?=>|\w+\s*=>)")
TS_CLASS = re.compile(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?(class|interface)\s+(\w+)")
TS_METHOD = re.compile(r"^\s*(?:(?:public|private|protected|static|async|override|readonly|abstract|get|set|declare)\s+)*\*?\s*(\w+)\s*\??\s*(?:<[^>]*>)?\s*\(")


class Def:
    __slots__ = ("name", "file", "line", "kind", "container", "trait", "lang", "is_iface")

    def __init__(self, name, file, line, kind, container=None, trait=None, lang=None, is_iface=False):
        self.name, self.file, self.line, self.kind = name, file, line, kind
        self.container, self.trait, self.lang, self.is_iface = container, trait, lang, is_iface

    def key(self):
        return f"{self.file}:{self.line}:{self.name}"

    def as_dict(self):
        return {k: getattr(self, k) for k in self.__slots__}


def scan_defs(path, rel, lang, cleaned):
    defs = []
    if lang == "py":
        stack = []  # (indent, class name, bases)
        for i, line in enumerate(cleaned, 1):
            if not line.strip():
                continue
            ind = len(line) - len(line.lstrip())
            while stack and ind <= stack[-1][0]:
                stack.pop()
            if stack and stack[-1][3] is None and ind > stack[-1][0]:
                stack[-1][3] = ind  # class body indentation
            m = PY_CLASS.match(line)
            if m:
                stack.append([len(m.group(1)), m.group(2), m.group(3), None])
                defs.append(Def(m.group(2), rel, i, "class", stack[-2][1] if len(stack) > 1 else None, lang=lang))
                continue
            m = PY_DEF.match(line)
            if m:
                is_method = bool(stack and ind == stack[-1][3])
                cont = stack[-1][1] if is_method else None
                iface = bool(is_method and re.search(r"\b(ABC|Protocol|ABCMeta)\b", stack[-1][2] or ""))
                kind = "method" if is_method else ("function" if ind == 0 else "local")
                defs.append(Def(m.group(2), rel, i, kind, cont, lang=lang, is_iface=iface))
    elif lang == "rs":
        depth = 0
        stack = []  # (depth_at_open, container, trait, is_trait)
        pending = None
        for i, line in enumerate(cleaned, 1):
            m = RS_IMPL_FOR.match(line)
            if m:
                pending = (m.group(2).split("::")[-1], m.group(1).split("<")[0].split("::")[-1].strip(), False)
            else:
                m = RS_IMPL.match(line)
                if m:
                    pending = (m.group(1).split("::")[-1].split("<")[0], None, False)
                else:
                    m = RS_TRAIT.match(line)
                    if m:
                        pending = (m.group(1), m.group(1), True)
            for m in RS_FN.finditer(line):
                cont = stack[-1] if stack and depth == stack[-1][0] + 1 else None
                defs.append(Def(m.group(1), rel, i, "method" if cont else "function",
                                cont[1] if cont else None, cont[2] if cont else None, lang,
                                is_iface=bool(cont and cont[3])))
            for ch in line:
                if ch == "{":
                    if pending is not None:
                        stack.append((depth, pending[0], pending[1], pending[2]))
                        pending = None
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    while stack and stack[-1][0] >= depth:
                        stack.pop()
            if pending is not None and line.rstrip().endswith(";"):
                pending = None
    elif lang == "go":
        iface = None
        for i, line in enumerate(cleaned, 1):
            if iface:
                if line.startswith("}"):
                    iface = None
                    continue
                m = GO_IFACE_M.match(line)
                if m and m.group(1) not in KEYWORDS:
                    defs.append(Def(m.group(1), rel, i, "method", iface, iface, lang, is_iface=True))
                continue
            m = GO_IFACE.match(line)
            if m:
                iface = m.group(1)
                defs.append(Def(m.group(1), rel, i, "interface", lang=lang))
                continue
            m = GO_METHOD.match(line)
            if m:
                defs.append(Def(m.group(2), rel, i, "method", m.group(1), lang=lang))
                continue
            m = GO_FUNC.match(line)
            if m:
                defs.append(Def(m.group(1), rel, i, "function", lang=lang))
    elif lang == "ts":
        depth = 0
        stack = []  # (depth_at_open, name, is_iface)
        pending = None
        for i, line in enumerate(cleaned, 1):
            m = TS_CLASS.match(line)
            in_class = stack and depth == stack[-1][0] + 1
            if m:
                pending = (m.group(2), m.group(1) == "interface")
                defs.append(Def(m.group(2), rel, i, m.group(1), lang=lang))
            else:
                m = TS_FUNC.match(line) or TS_ARROW.match(line)
                if m and not in_class:
                    defs.append(Def(m.group(1), rel, i, "function", lang=lang))
                elif in_class:
                    m = TS_METHOD.match(line)
                    if m and m.group(1) not in KEYWORDS:
                        s = line.rstrip()
                        is_decl = s.endswith("{") or s.endswith(";") or s.endswith(",") or s.endswith("(") \
                            or re.search(r"\)\s*(:[^;{]*)?\s*(\{|;|=>)", s)
                        if is_decl:
                            defs.append(Def(m.group(1), rel, i, "method", stack[-1][1], lang=lang,
                                            is_iface=stack[-1][2] or bool(re.match(r"^\s*abstract\b", line))))
            for ch in line:
                if ch == "{":
                    if pending is not None:
                        stack.append((depth, pending[0], pending[1]))
                        pending = None
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    while stack and stack[-1][0] >= depth:
                        stack.pop()
    return defs


# ---------------------------------------------------------------- corpus model
class Corpus:
    def __init__(self, root):
        self.root = root
        self.files = list_files(root)
        self.cleaned = {}
        self.raw = {}
        self.defs = []
        self.by_name = defaultdict(list)
        self.call_counts = defaultdict(int)
        self.imports = {}
        call_re = re.compile(r"\b([A-Za-z_]\w*)\s*\(")
        for rel in self.files:
            lang = lang_of(rel)
            try:
                with open(os.path.join(root, rel), "rb") as f:
                    data = f.read()
            except OSError:
                continue
            if len(data) > 2_000_000 or b"\0" in data[:8192]:
                continue
            text = data.decode("utf-8", "replace")
            cl = clean_lines(text, lang)
            self.cleaned[rel] = cl
            self.raw[rel] = text.split("\n")
            for d in scan_defs(os.path.join(root, rel), rel, lang, cl):
                self.defs.append(d)
                self.by_name[d.name].append(d)
            for line in cl:
                for m in call_re.finditer(line):
                    self.call_counts[m.group(1)] += 1

    def callable_defs(self, name):
        return [d for d in self.by_name.get(name, []) if d.kind in ("function", "method", "local")]

    def call_sites(self, name):
        """All (file, line, qualifier, text) where `name` is invoked (strings/comments blanked)."""
        pat = re.compile(r"(::\s*|\.\s*|->\s*)?(?<![\w$])" + re.escape(name) +
                         r"(\s*::\s*<[^>]*>)?\s*(?:<[^<>()]*>)?\s*\(")
        sites = []
        for rel, cl in self.cleaned.items():
            lang = lang_of(rel)
            defs_here = {d.line for d in self.by_name.get(name, []) if d.file == rel}
            for i, line in enumerate(cl, 1):
                if name not in line:
                    continue
                if i in defs_here:
                    continue
                for m in pat.finditer(line):
                    before = line[:m.start()].rstrip()
                    if lang == "py" and re.search(r"\b(def|class)\s*$", before):
                        continue
                    if lang == "rs" and re.search(r"\bfn\s*$", before):
                        continue
                    if lang == "go" and re.search(r"\bfunc\s*(\([^)]*\)\s*)?$", before):
                        continue
                    if lang == "ts":
                        if re.search(r"\bfunction\s*\*?\s*$", before):
                            continue
                        s = line.strip()
                        if s.startswith(name) and re.search(r"\)\s*(:[^;{=]*)?\s*\{\s*$", s):
                            continue  # method definition
                    if lang == "rs" and m.end() < len(line) and line[m.start():m.end()].rstrip().endswith("!("):
                        continue
                    q = (m.group(1) or "").strip()
                    recv = ""
                    if q in (".", "->", "::"):
                        mm = re.search(r"([\w$]+)\s*(?:\(\s*\))?\s*$", line[:m.start()])
                        recv = mm.group(1) if mm else ""
                    sites.append({"file": rel, "line": i, "qual": q, "recv": recv,
                                  "text": self.raw[rel][i - 1].strip()[:200]})
                    break
        return sites

    def enclosing_class(self, rel, line):
        best = None
        for d in self.defs:
            if d.file == rel and d.line <= line and d.kind in ("class", "interface") and \
                    (best is None or d.line > best.line):
                best = d
        return best.name if best else None

    def imported_from(self, rel, name):
        """Module path hint for `name` imported in file rel (py/ts/rs), else None."""
        lang = lang_of(rel)
        for line in self.cleaned.get(rel, []):
            if name not in line:
                continue
            if lang == "py":
                m = re.match(r"^\s*from\s+([\w.]+)\s+import\s+.*\b" + re.escape(name) + r"\b", line)
                if m:
                    return ("py", m.group(1))
            elif lang == "ts":
                raw = self.raw[rel][self.cleaned[rel].index(line)]
                m = re.search(r"import\s*\{[^}]*\b" + re.escape(name) + r"\b[^}]*\}\s*from\s*['\"]([^'\"]+)", raw)
                if m:
                    return ("ts", m.group(1))
            elif lang == "rs":
                m = re.match(r"^\s*(?:pub\s+)?use\s+([\w:]+)::(?:\{[^}]*\b" + re.escape(name) + r"\b|" + re.escape(name) + r"\b)", line)
                if m:
                    return ("rs", m.group(1))
        return None

    def resolve(self, site, name):
        """Return the Def a call site binds to, 'other' (not a callable def we know), or None (ambiguous)."""
        cands = self.callable_defs(name)
        if not cands:
            return "other"
        rel, lang, q, recv = site["file"], lang_of(site["file"]), site["qual"], site["recv"]
        if q in (".", "->") and recv not in ("self", "this", "Self") and name in STD_METHODS:
            return None  # could be a standard-library method on a foreign type
        if len(cands) == 1:
            return cands[0]
        same_lang = [d for d in cands if d.lang == lang]
        if len(same_lang) == 1:
            return same_lang[0]
        if not q:
            if lang == "go":
                pkg = os.path.dirname(rel)
                loc = [d for d in same_lang if d.kind == "function" and os.path.dirname(d.file) == pkg]
            else:
                loc = [d for d in same_lang if d.kind in ("function", "local") and d.file == rel]
            if len(loc) == 1:
                return loc[0]
            imp = self.imported_from(rel, name)
            if imp:
                kind, mod = imp
                if kind == "py":
                    tail = mod.replace(".", "/")
                    hit = [d for d in same_lang if d.kind == "function" and
                           (d.file[:-3].endswith(tail) or d.file.endswith(tail + "/__init__.py"))]
                elif kind == "ts":
                    base = os.path.normpath(os.path.join(os.path.dirname(rel), mod)) if mod.startswith(".") else mod
                    hit = [d for d in same_lang if os.path.splitext(d.file)[0] in (base, base + "/index")]
                else:
                    seg = mod.split("::")[-1]
                    hit = [d for d in same_lang if d.kind == "function" and
                           (f"/{seg}.rs" in "/" + d.file or f"/{seg}/mod.rs" in "/" + d.file)]
                if len(hit) == 1:
                    return hit[0]
            return None
        if recv in ("self", "this", "Self"):
            cls = self.enclosing_class(rel, site["line"])
            hit = [d for d in same_lang if d.file == rel and d.kind == "method" and
                   (cls is None or d.container == cls)]
            if lang == "rs":
                hit = [d for d in same_lang if d.file == rel and d.kind == "method"]
            if len(hit) == 1:
                return hit[0]
            return None
        if q == "::" or (recv[:1].isupper() and lang in ("py", "ts")):
            hit = [d for d in same_lang if d.container == recv]
            if len(hit) == 1:
                return hit[0]
            if q == "::" and lang == "rs":
                hit = [d for d in same_lang if d.kind == "function" and
                       (f"/{recv}.rs" in "/" + d.file or f"/{recv}/mod.rs" in "/" + d.file)]
                if len(hit) == 1:
                    return hit[0]
            if lang == "go":
                hit = [d for d in same_lang if d.kind == "function" and os.path.basename(os.path.dirname(d.file)) == recv]
                if len(hit) == 1:
                    return hit[0]
            return None
        if name in STD_METHODS:
            return None
        if lang == "go" and recv:
            hit = [d for d in same_lang if d.kind == "function" and os.path.basename(os.path.dirname(d.file)) == recv]
            if len(hit) == 1:
                return hit[0]
        methods = [d for d in same_lang if d.kind == "method"]
        if len(methods) == 1:
            return methods[0]
        return None
