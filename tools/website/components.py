"""HTML building blocks shared by pages: terminal transcripts, code blocks, tables."""
from __future__ import annotations

import json
import re

import highlight
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


def command_html(record: dict) -> str:
    """The command of a run, highlighted; the prompt span (.p) stays outside the copied text."""
    return '<span class="p">$ </span>' + highlight.highlight(command(record), "sh")


def meta_line(record: dict) -> str:
    ok = record["exit"] == 0
    return (f'<span class="ln meta"># <b{"" if ok else " class=\"bad\""}>exit {record["exit"]}</b> &middot; '
            f'{fmt(record["bytes"])} bytes &middot; ~{fmt(record["est_tokens"])} est. tokens</span>')


def transcript(record: dict, stdout: str | None = None, limit: int | None = None, more: str | None = None,
               note: str | None = None, lang: str | None = None) -> str:
    """The lines of one captured run: the command, stdout, stderr and the facts line."""
    lines = [f'<span class="ln cmd">{command_html(record)}</span>']
    if note:
        lines.append(f'<span class="ln out hdrl">{e(note)}</span>')
    text = record["stdout"] if stdout is None else stdout
    body = text[:-1] if text.endswith("\n") else text
    out = highlight.lines(body, lang or highlight.output_lang(record["argv"], body)) if body else []
    shown = out[:limit] if limit else out
    lines += [f'<span class="ln out">{t}</span>' for t in shown]
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
    """A labelled code block. Shell lines get a prompt (not copied); comment lines stay as they are."""
    if lang == "shell":
        text = "\n".join(line if not line.strip() or line.lstrip().startswith("#") else "$ " + line for line in text.split("\n"))
        lang = "sh"
    body = highlight.highlight(text, lang)
    return (f'<div class="code"><div class="code-head"><span>{e(label)}</span><button class="copy" type="button" hidden>Copy</button></div>'
            f'<pre><code>{body}</code></pre></div>')


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
    from glyphs import icon
    sort_icon = icon(0x2191, "sort-icon") if sortable else ""
    th = "".join(f'<th scope="col"{" class=\"num\"" if i in numeric else ""}{sort}>{cell(h)}{sort_icon}</th>' for i, h in enumerate(head))
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
