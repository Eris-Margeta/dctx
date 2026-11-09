"""The home showcase: five ways to use dctx, each a plain-language step beside the real command and its output.

Every terminal line is captured data from data/demo.json (capture.py on sample/); long outputs are cut to
excerpts with an exact "… N lines" marker, never retyped. The page carries every step and every panel in
full: without JS the steps read as a list and the panels follow one another; with JS (src/hero.js) the
panels share one cell of a grid, so the tallest one sets the height and switching never moves the page."""
from __future__ import annotations

import json

import highlight
from common import VERSION, e, rec
from components import command_html, meta_line
from pages.paths import md_inline


def _lines(record: dict, keep: list[tuple[int, int]] | None = None, stdout: str | None = None,
           lang: str | None = None) -> list[str]:
    """The command, the kept stdout ranges (inclusive, highlighted over the whole text), a marker for each gap,
    and the facts line. Without ranges the whole stdout is shown."""
    text = record["stdout"] if stdout is None else stdout
    body = text[:-1] if text.endswith("\n") else text
    out = highlight.lines(body, lang or highlight.output_lang(record["argv"], body))
    keep = keep or [(0, len(out) - 1)]
    rows = [("cmd", command_html(record))]
    at = 0
    for a, b in keep:
        if a > at:
            rows.append(("gap", f"… {a - at} lines"))
        rows += [("out", t) for t in out[a:b + 1]]
        at = b + 1
    if at < len(out):
        rows.append(("gap", f"… {len(out) - at} more lines"))
    return [f'<span class="ln {c}">{t or " "}</span>' if c != "gap" else f'<span class="ln out hdrl">{e(t)}</span>'
            for c, t in rows] + [meta_line(record)]


def _pack() -> list[str]:
    return _lines(rec("pack"), [(0, 1), (11, 14), (28, 30)])


def _task() -> list[str]:
    return _lines(rec("pack-for"), [(0, 3), (14, 18), (26, 26)])


def _agent() -> list[str]:
    return _lines(rec("find")) + ['<span class="ln gap"></span>'] + _lines(rec("read"))


def _mcp() -> list[str]:
    r = rec("mcp")
    calls = r["stdin"].rstrip("\n").split("\n")
    replies = r["stdout"].rstrip("\n").split("\n")
    # Re-serialised with a space after each separator, so a phone wraps between tokens; same JSON values.
    ask = highlight.lines(json.dumps(json.loads(calls[-1]), ensure_ascii=False, separators=(", ", ": ")), "json")[0]
    answer = highlight.lines(json.dumps(json.loads(replies[-1]), ensure_ascii=False, separators=(", ", ": ")), "json")[0]
    return [f'<span class="ln cmd">{command_html(r)}</span>',
            f'<span class="ln out hdrl"># the agent calls find (stdin, line {len(calls)} of {len(calls)})</span>',
            f'<span class="ln out in">{ask}</span>',
            f'<span class="ln out hdrl"># dctx answers (stdout, reply {len(replies)} of {len(replies)})</span>',
            f'<span class="ln out">{answer}</span>',
            meta_line(r)]


def _library() -> list[str]:
    r = rec("read-json")
    pretty = json.dumps(json.loads(r["stdout"]), ensure_ascii=False, indent=1) + "\n"
    rows = _lines(r, [(0, 3), (13, 17), (29, 29), (38, 38)], stdout=pretty, lang="json")
    return rows[:1] + ['<span class="ln out hdrl"># one JSON line; pretty-printed here</span>'] + rows[1:]


# (id, short label, title, one plain sentence (backticks become code), link, link text, panel lines)
STEPS = [
    ("chat", "Chat", "Paste a whole project into a chat",
     "One command turns a folder into one page you can paste into any AI chat. Secret files such as `.env` "
     "are listed by name and never read.", "/cli/#pack-a-project", "Pack a project", _pack),
    ("task", "Task", "Hand over only what a task needs",
     "Name the task in a few words and set a size limit. dctx picks the matching code, what calls it and its "
     "source, and stops at the limit.", "/cli/#pack-for-a-task", "Pack for a task", _task),
    ("agent", "Agent", "Let an agent explore a big repo",
     "The agent searches by name, gets short stable ids back, then reads only the piece it asks for. "
     "Same question, same answer, every turn.", "/docs/map-mode/", "Map mode guide", _agent),
    ("mcp", "MCP", "Plug into Claude Code, Cursor, Codex",
     "Add one MCP server, `dctx mcp --profile lean`, and your agent gets two tools: find and read. "
     "MCP works today over stdio; ACP is planned.", "/docs/modalities/#client-configuration", "Client setup", _mcp),
    ("lib", "Library", "Build it into your own tool",
     "Ask for `--json` and get structured records with the exact source. The Rust library works from Git "
     "today; Python, Node and Go wrappers are built in CI, registry publication pending.",
     "/docs/modalities/#rust-library-embed-the-facade", "Embed the library", _library),
]


def tour() -> str:
    tabs, panels = [], []
    for i, (sid, label, title, line, link, link_text, lines) in enumerate(STEPS):
        on = i == 0
        tabs.append(
            f'<li class="tour-step{" on" if on else ""}"><button class="tour-tab" type="button" role="tab" id="tour-t-{sid}" '
            f'aria-controls="tour-p-{sid}" aria-selected="{"true" if on else "false"}"{"" if on else " tabindex=\"-1\""}>'
            f'<span class="tour-n" aria-hidden="true">{i + 1}</span><span class="tour-k">{e(label)}</span>'
            f'<span class="tour-title">{e(title)}</span><span class="tour-bar" aria-hidden="true"><i></i></span></button>'
            f'<p class="tour-line">{md_inline(line)} <a href="{link}">{e(link_text)}</a></p></li>')
        body = "".join(lines())
        panels.append(
            f'<div class="tour-panel{" on" if on else ""}" role="tabpanel" id="tour-p-{sid}" aria-labelledby="tour-t-{sid}" tabindex="0">'
            f'<p class="tour-cap"><b>{e(title)}</b> {md_inline(line)}</p>'
            f'<div class="term tour-term"><div class="term-bar"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>'
            f'<span class="title">~/kiosk &middot; dctx {VERSION}</span><span class="tour-of">{i + 1}/{len(STEPS)}</span></div>'
            f'<pre>{body}</pre></div></div>')
    return f"""<div class="tour" data-tour>
 <div class="tour-side">
  <p class="eyebrow tour-eyebrow" id="tour-h">Five ways to use it</p>
  <ol class="tour-steps" role="tablist" aria-labelledby="tour-h">{"".join(tabs)}</ol>
 </div>
 <div class="tour-screen">{"".join(panels)}</div>
 <p class="tour-note">Real commands and output, recorded from the dctx {VERSION} binary on the <a href="/examples/">kiosk sample project</a>; long output is cut where it says &hellip;</p>
</div>"""
