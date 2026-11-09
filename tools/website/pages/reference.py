"""/cli/ and /languages/: generated from the captured binary output, nothing typed by hand."""
from __future__ import annotations

import re

import data
import highlight
from common import ORG_ID, ORIGIN, PERSON_ID, APP_ID, VERSION, e, read, rec, src
from components import output_examples, table
from routes import doc_path

VERBS = ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]


def pre(text: str, lang: str = "help") -> str:
    return (f'<div class="code"><div class="code-head"><span>verbatim</span><button class="copy" type="button" hidden>Copy</button></div>'
            f'<pre><code>{highlight.highlight(text, lang)}</code></pre></div>')


def cli() -> tuple[str, list]:
    help_sections = data.help_sections(rec("help")["stdout"])
    sec_html = []
    nav = []
    for title, text in help_sections:
        hid = "help" if title == "Usage" else "help-" + re.sub(r"[^a-z]+", "-", title.lower()).strip("-")
        nav.append((hid, "dctx --help" if title == "Usage" else title))
        sec_html.append(f'<section class="help-sec" id="{hid}"><h3>{e(title)}</h3>{pre(text)}</section>')
    verbs = []
    for verb in VERBS:
        r = rec(f"help-{verb}")
        verbs.append(f'<section class="help-sec" id="{verb}"><h3><code>dctx map {verb}</code></h3>{pre(r["stdout"].rstrip(chr(10)))}'
                     f'<p class="note"><a href="/examples/">Examples on the sample</a></p></section>')
    env = data.env_vars()
    env_rows = [[f"<code>{v}</code>", e(text, quote=False).replace("`", ""), f'<code>dctx {"map --help" if rid == "map-help" else "--help"}</code>']
                for v, text, rid in env]
    exit_line, exit_head, exit_rows = data.md_tables(read("docs/CLI-EXIT-CODES.md"))[0]
    tools = data.mcp_tools()
    tool_rows = []
    for t in tools:
        props = t["inputSchema"].get("properties", {})
        req = set(t["inputSchema"].get("required", []))
        params = "<br>".join(f'<code>{e(k)}</code>{"*" if k in req else ""} <span class="muted">{e(v.get("type", ""))}</span> {e(v.get("description", ""))}'
                             for k, v in props.items())
        tool_rows.append([f"<code>{e(t['name'])}</code>", e(t["description"]), params])
    token_line = next(l for l in rec("help")["stdout"].split("\n") if l.startswith("token estimate:"))
    nav += [("map-help", "dctx map --help")] + [(v, v) for v in VERBS] + [("languages", "languages"), ("env", "environment"),
                                                                              ("exit-codes", "exit codes"), ("mcp", "MCP tools"), ("tokens", "token estimate")]
    nav_html = '<ul class="toc-row">' + "".join(f'<li><a href="#{i}">{e(t)}</a></li>' for i, t in nav) + "</ul>"
    langs = data.languages()
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Reference</span><h1>CLI reference</h1>
<p class="lede">Captured from <code>dctx {VERSION}</code> when this site was built; a check fails the build when the binary prints anything else. Help text is shown verbatim, so it is the CLI's own wording, including the legacy snapshot options it still lists.</p></div>
{output_examples()}
<p><a href="/docs/modalities/">Usage modes and MCP client configuration</a> &middot;
<a href="/docs/readme/#use-as-a-library">Rust library and publication status</a></p>
{nav_html}
<h2 id="dctx-help">dctx --help</h2>
{"".join(sec_html)}
<h2 id="map-help">dctx map --help</h2>
{pre(rec("map-help")["stdout"].rstrip(chr(10)))}
<h2 id="verbs">Map verbs</h2>
{"".join(verbs)}
<h2 id="languages">dctx languages</h2>
{pre(rec("languages-help")["stdout"].rstrip(chr(10)))}
<p>{langs["total"]} languages: {langs["tiers"]["full"]} full, {langs["tiers"]["outline"]} outline, {langs["tiers"]["docs"]} docs. The table with every tag and file name is on <a href="/languages/">/languages/</a>.</p>
<h2 id="env">Environment variables</h2>
{table(["Variable", "Help text", "Printed by"], env_rows, raw=True)}
<h2 id="exit-codes">Exit codes</h2>
{table(exit_head, exit_rows, caption=src("docs/CLI-EXIT-CODES.md", "CLI-EXIT-CODES.md", exit_line) + ' &middot; <a href="' + doc_path("exit-codes") + '">full contract</a>')}
<h2 id="mcp">MCP tools</h2>
<p><code>dctx mcp</code> answers <code>tools/list</code> with {len(tools)} tools (captured over stdio; <code>*</code> marks a required argument). <a href="/examples/#mcp">The full exchange</a>.</p>
{table(["Tool", "Description", "Arguments"], tool_rows, raw=True)}
<h2 id="tokens">Token estimate</h2>
<p>Every byte and token count on this site uses the estimate dctx itself prints:</p>
{pre(token_line)}
</div>"""
    url = ORIGIN + "/cli/"
    article = {"@type": "TechArticle", "@id": url + "#article", "headline": "dctx CLI reference", "url": url, "inLanguage": "en",
               "about": {"@id": APP_ID}, "author": {"@id": PERSON_ID}, "publisher": {"@id": ORG_ID},
               "mainEntityOfPage": {"@id": url + "#webpage"}, "description": "Every dctx command, flag, environment variable, exit code and MCP tool."}
    return body, [article]


def languages() -> str:
    langs = data.languages()
    tiers = data.readme_tiers()
    rows = []
    for row in langs["rows"]:
        rows.append([e(row[0]), f"<code>{e(row[1].strip('`'))}</code>", e(row[2], quote=False).replace("`", ""),
                     f'<span class="tier-{e(row[3])}">{e(row[3])}</span>', e(row[4])])
    import md as markdown_mod
    tier_html, _, _ = markdown_mod.to_html(tiers, "README.md", markdown_mod.Linker({}))
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Reference</span><h1>Languages</h1>
<p class="lede">{langs["total"]} language families in three tiers: {langs["tiers"]["full"]} full, {langs["tiers"]["outline"]} outline and {langs["tiers"]["docs"]} documentation formats. The table is the output of <code>dctx languages --markdown</code>, captured from dctx {VERSION}.</p></div>
<div class="cols cols-12"><div class="prose">{tier_html}<p class="note">{src("README.md", "README: Language tiers")}</p></div>
<div>{table(["Language", "Tag", "Files", "Tier", "Confidence"], rows, raw=True, sortable=True, caption="Click a column heading to sort.")}</div></div></div>"""
