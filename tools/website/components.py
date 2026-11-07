"""HTML building blocks shared by pages: terminal transcripts, code blocks, tables."""
from __future__ import annotations

import json
import re

from common import demo, e, fmt

SAFE = re.compile(r"^[A-Za-z0-9_./=,:@%+-]+$")


def output_examples() -> str:
    """The same captured output section as README, rendered as site Markdown."""
    import md
    from output_examples import render

    body, _, _ = md.to_html(render(demo()["records"]), "README.md", md.Linker({}))
    return '<div class="prose">' + body + '</div>'


def quote(arg: str) -> str:
    """Display an argument the way a shell user would type it."""
    if arg and SAFE.match(arg):
        return arg
    return "'" + arg.replace("'", "'\\''") + "'"


def command(record: dict) -> str:
    return " ".join(quote(a) for a in record["argv"])


def out_line(text: str) -> str:
    """One output line with record ids and header lines marked up."""
    if text.startswith("#"):
        return f'<span class="ln out hdrl">{e(text)}</span>'
    m = re.match(r"^((?:calls|imports|contains):(?:in|out):)?([smdkp]:\S+?)(\t|$)", text)
    if m:
        prefix, ident = m.group(1) or "", m.group(2)
        rest = text[m.end(2):]
        return f'<span class="ln out"><span class="cm">{e(prefix)}</span><span class="id">{e(ident)}</span>{e(rest)}</span>'
    return f'<span class="ln out">{e(text)}</span>'


def meta_line(record: dict) -> str:
    ok = record["exit"] == 0
    return (f'<span class="ln meta"># <b{"" if ok else " class=\"bad\""}>exit {record["exit"]}</b> &middot; '
            f'{fmt(record["bytes"])} bytes &middot; ~{fmt(record["est_tokens"])} est. tokens</span>')


def transcript(record: dict, stdout: str | None = None, limit: int | None = None, more: str | None = None,
               note: str | None = None) -> str:
    """The lines of one captured run: the command, stdout, stderr and the facts line."""
    lines = [f'<span class="ln cmd"><span class="p">$ </span>{e(command(record))}</span>']
    if note:
        lines.append(f'<span class="ln out hdrl">{e(note)}</span>')
    text = record["stdout"] if stdout is None else stdout
    body = text[:-1] if text.endswith("\n") else text
    out = body.split("\n") if body else []
    shown = out[:limit] if limit else out
    lines += [out_line(t) for t in shown]
    if limit and len(out) > limit:
        lines.append(f'<span class="ln out hdrl">… {len(out) - limit} more lines{(" · " + more) if more else ""}</span>')
    if record["stderr"]:
        lines += [f'<span class="ln err">{e(t)}</span>' for t in record["stderr"].rstrip("\n").split("\n")]
    lines.append(meta_line(record))
    return "".join(lines)


def term(inner: str, title: str, *, play: str | None = None, cls: str = "", replay: bool = True) -> str:
    attrs = f' data-play="{play}"' if play else ""
    btn = '<button class="tbtn" type="button" data-replay hidden>Replay</button>' if play and replay else ""
    return (f'<div class="term {cls}"{attrs}><div class="term-bar"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>'
            f'<span class="title">{e(title)}</span>{btn}</div><div class="scroll"><pre>{inner}</pre></div></div>')


def code(label: str, text: str, lang: str = "") -> str:
    lines = []
    for raw in text.split("\n"):
        m = re.match(r"^(.*?)(\s+#\s.*)?$", raw)
        cmd, comment = m.group(1), m.group(2) or ""
        if raw.lstrip().startswith("#"):
            lines.append(f'<span class="t-c1">{e(raw)}</span>')
        elif lang == "shell":
            lines.append(f'<span class="p">$ </span>{e(cmd)}' + (f'<span class="t-c1">{e(comment)}</span>' if comment else ""))
        else:
            lines.append(e(raw))
    return (f'<div class="code"><div class="code-head"><span>{e(label)}</span><button class="copy" type="button" hidden>Copy</button></div>'
            f'<pre><code>{chr(10).join(lines)}</code></pre></div>')


def table(head: list[str], rows: list[list[str]], *, caption: str = "", numeric: set[int] = frozenset(), raw: bool = False,
          sortable: bool = False, cls: str = "") -> str:
    """A table from parsed cells. Cells are Markdown-ish: `code` and **bold** are kept."""
    def cell(text: str) -> str:
        if raw:
            return text
        text = e(text, quote=False)
        text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
        return re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    sort = ' data-sort=""' if sortable else ""
    th = "".join(f'<th scope="col"{" class=\"num\"" if i in numeric else ""}{sort}>{cell(h)}</th>' for i, h in enumerate(head))
    trs = "".join("<tr>" + "".join(f'<td{" class=\"num\"" if i in numeric else ""}>{cell(c)}</td>' for i, c in enumerate(r)) + "</tr>" for r in rows)
    cap = f"<caption>{caption}</caption>" if caption else ""
    return (f'<div class="table-wrap"><table{" data-sortable" if sortable else ""}{f" class={chr(34)}{cls}{chr(34)}" if cls else ""}>'
            f"{cap}<thead><tr>{th}</tr></thead><tbody>{trs}</tbody></table></div>")


def pretty(record_stdout: str) -> str:
    """JSON lines pretty-printed with one-space indent (what /examples/ shows by default)."""
    try:
        return json.dumps(json.loads(record_stdout), ensure_ascii=False, indent=1) + "\n"
    except ValueError:
        pass
    out = []
    for line in record_stdout.rstrip("\n").split("\n"):
        if line.strip():
            out.append(json.dumps(json.loads(line), ensure_ascii=False, indent=1))
    return "\n".join(out) + "\n"
