"""/examples/ and /templates/: every captured record, and the two built-in templates."""
from __future__ import annotations

import difflib
import json
import re

import highlight
from common import BLOB, ORIGIN, VERSION, demo, e, fmt, read, rec, src
from components import command, pretty, term, transcript

GROUPS = [("pack", "Pack"), ("map", "Map, text"), ("json", "Map, JSON"), ("graph", "Graph export"),
          ("errors", "Errors and exit codes")]
TEMPLATE_DIR = "crates/dctx-template/templates"


def badges(r: dict) -> str:
    ok = "ok" if r["exit"] == 0 else "bad"
    return (f'<div class="badges"><span class="{ok}">exit {r["exit"]}</span><span>{fmt(r["bytes"])} bytes</span>'
            f'<span>~{fmt(r["est_tokens"])} est. tokens</span></div>')


def block(r: dict) -> str:
    note = f'<p class="note">{re_code(r["note"])}</p>' if r.get("note") else ""
    is_json = r["group"] == "json" or r["id"] == "export-json"
    if is_json and "scrubbed" not in r:
        shown = term(transcript(r, stdout=pretty(r["stdout"]), note="# pretty-printed; the raw bytes are below"), "kiosk")
        raw = f'<details class="raw"><summary><svg class="disclosure" width="10" height="10" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" focusable="false"><path d="m2 1 6 4-6 4z"/></svg>Raw output ({fmt(r["bytes"])} bytes, as captured)</summary><pre>{highlight.highlight(r["stdout"].rstrip(chr(10)), "json")}</pre></details>'
    else:
        shown = term(transcript(r), "kiosk")
        raw = ""
    stdin = ""
    if r.get("stdin"):
        stdin = f'<details class="raw"><summary><svg class="disclosure" width="10" height="10" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" focusable="false"><path d="m2 1 6 4-6 4z"/></svg>stdin ({len(r["stdin"].splitlines())} lines)</summary><pre>{highlight.highlight(r["stdin"].rstrip(chr(10)), "json")}</pre></details>'
    return (f'<section class="ex" id="{r["id"]}"><div class="ex-head"><h3><a href="#{r["id"]}">{e(command(r))}</a></h3>{badges(r)}</div>'
            f"{note}{stdin}{shown}{raw}</section>")


def re_code(text: str) -> str:
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text, quote=False))


def sample_table() -> str:
    rows = "".join(f'<tr><td><code>{e(f["path"])}</code></td><td class="num">{f["bytes"]}</td><td>{e(f["status"])}</td></tr>'
                   for f in demo()["sample"]["files"])
    return (f'<div class="table-wrap"><table><caption>The {len(demo()["sample"]["files"])} files of the sample, and what <code>dctx pack .</code> '
            f'did with each (derived from the captured runs). {src("tools/website/sample", "tools/website/sample")}</caption>'
            f'<thead><tr><th scope="col">File</th><th scope="col" class="num">Bytes</th><th scope="col">In pack</th></tr></thead><tbody>{rows}</tbody></table></div>')


def examples() -> tuple[str, list]:
    d = demo()
    sections, nav, parts = [], [], []
    for group, title in GROUPS:
        records = [r for r in d["records"] if r["group"] == group]
        nav.append(f'<li><a href="#g-{group}">{e(title)} ({len(records)})</a></li>')
        sections.append(f'<h2 id="g-{group}">{e(title)}</h2>' + "".join(block(r) for r in records))
        for r in records:
            parts.append({"@type": "SoftwareSourceCode", "@id": ORIGIN + "/examples/#" + r["id"], "name": command(r),
                          "programmingLanguage": "Shell", "codeSampleType": "code snippet", "text": command(r),
                          "runtimePlatform": f"dctx {VERSION}"})
    env = ", ".join(f"{k}={v}" for k, v in d["env"].items())
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Examples</span><h1>Examples</h1>
<p class="lede">{len(d["records"])} runs of dctx {VERSION} on one small project, recorded when this site was built: the exact stdout and stderr, the exit code, the bytes and the estimated tokens (<code>ceil(2 × bytes / 7)</code>). Each run starts from an empty home, config and cache ({e(env)}), in a copy of the sample named <code>kiosk</code>. Wall time is not recorded.</p></div>
<div class="cols cols-12"><div>{sample_table()}</div><div><p>Only three values are not stable between runs and are replaced with <code>…</code>, each where it occurs: the commit hash in <code>--version</code>, the <code>MOD:</code> times of <code>--tree-stamps=on</code> and <code>age_s</code> of <code>--meta</code>. The help pages are on <a href="/cli/">/cli/</a>, the template builds on <a href="/templates/">/templates/</a>. The same commands run live in the <a href="/playground/">playground</a>.</p>
<ul class="toc-row">{"".join(nav)}</ul></div></div>
{"".join(sections)}</div>"""
    return body, [{"@merge": True, "hasPart": [{"@id": p["@id"]} for p in parts]}] + parts


def highlight_dts(text: str) -> str:
    return highlight.highlight(text, "dts")


def tree_lines(stdout: str) -> list[str]:
    m = re.search(r"<DIRECTORY_TREE>\n(.*?)</DIRECTORY_TREE>", stdout, re.S)
    return m.group(1).rstrip("\n").split("\n")


def word_diff(a: str, b: str) -> str:
    sa, sb = re.split(r"(\s+|,)", a), re.split(r"(\s+|,)", b)
    out = []
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, sa, sb, autojunk=False).get_opcodes():
        if op == "equal":
            out.append(e("".join(sa[i1:i2])))
        if op in ("delete", "replace"):
            out.append(f"<del>{e(''.join(sa[i1:i2]))}</del>")
        if op in ("insert", "replace"):
            out.append(f"<ins>{e(''.join(sb[j1:j2]))}</ins>")
    return "".join(out)


def templates() -> tuple[str, list]:
    default_src = read(f"{TEMPLATE_DIR}/default.dctx_template")
    stamped_src = read(f"{TEMPLATE_DIR}/stamped.dctx_template")
    core_copy = read("crates/dctx-core/templates/default.dctx_template")
    if core_copy != default_src:
        raise SystemExit("templates: crates/dctx-core and crates/dctx-template default templates differ")
    pack, stamped = rec("pack"), rec("pack-stamps")
    a, b = tree_lines(pack["stdout"]), tree_lines(stamped["stdout"])
    diff = "\n".join(word_diff(x, y) for x, y in zip(a, b))
    purpose = default_src.split("\n")[3].lstrip("# ").strip()
    selection = read("crates/dctx-template/src/legacy_output.rs")
    sel_line = next(i for i, l in enumerate(selection.split("\n"), 1) if "stamped.dctx_template" in l)
    adr = read("docs/ADR-003-Templating-Engine.md")
    import md as md_mod
    guide_parts = []
    for heading in ("## 1. Decision: Explicit Marker Definition", "## 3. Decision: Two-Pass Rendering Engine",
                    "## 4. Template File Structure", "## 5. Available Data Variables (Runtime)"):
        start = adr.index(heading)
        end = adr.find("\n## ", start + 4)
        end2 = adr.find("\n6. TEMPLATE DEFINITION", start)
        stop = min(x for x in (end, end2, len(adr)) if x > 0)
        guide_parts.append(adr[start:stop].replace("## ", "### ", 1))
    guide_html, _, _ = md_mod.to_html("\n\n".join(guide_parts), "docs/ADR-003-Templating-Engine.md", md_mod.Linker({
        "docs/ADR-003-Templating-Engine.md": "/docs/templating/"}))
    t_def, t_stamp = rec("template-default"), rec("template-stamped")

    def source_block(name: str, text: str) -> str:
        return (f'<div class="code"><div class="code-head"><span>{TEMPLATE_DIR}/{name}</span><a class="src" href="{BLOB}{TEMPLATE_DIR}/{name}">source</a>'
                f'<button class="copy" type="button" hidden>Copy</button></div><pre><code>{highlight_dts(text)}</code></pre></div>')
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Templates</span><h1>Templates</h1>
<p class="lede">The dctx Template System (DTS) defines the text <code>dctx pack</code> prints. dctx {VERSION} ships two built-in templates; both are shown here with their real output on the sample project.</p></div>
<section class="sec" aria-labelledby="which-h"><h2 id="which-h">Which template pack uses</h2>
<p><code>dctx pack</code> renders the <b>default</b> template; <code>--tree-stamps=on</code> selects the <b>stamped</b> one, the 1.0.0 text. {src("crates/dctx-template/src/legacy_output.rs", "legacy_output.rs", sel_line)}
<code>dctx template build &lt;PATH&gt;</code> parses and validates a template file and prints its compiled plan; with <code>--output PATH</code> it writes the plan. <code>dctx template list</code> lists explicit compiled plans. The 1.1.0 <code>pack</code> command has no option that selects a compiled plan, so this page shows no third, custom template.</p>
{term(transcript(t_def) + '<span class="ln gap"></span>' + transcript(t_stamp), "dctx template build")}
</section>
<section class="sec" aria-labelledby="diff-h"><h2 id="diff-h">What changes between them</h2>
<p>{e(purpose)} The only difference is the tree entry and its instruction line: the default leaves out <code>MOD</code> and <code>SIZE</code>, because a timestamp changes on a bare <code>touch</code> and a size on every edit, and either breaks the byte-stable prefix a prompt cache reuses. The tree of the sample, word by word (<del>default</del> to <ins>stamped</ins>; the MOD digits are replaced with … since they are file times):</p>
<div class="code diff"><div class="code-head"><span>DIRECTORY_TREE: pack . → pack . --tree-stamps=on</span></div><pre><code>{diff}</code></pre></div>
</section>
<section class="sec" aria-labelledby="default-h"><h2 id="default-h">default.dctx_template</h2>
<div class="cols cols-2"><div>{source_block("default.dctx_template", default_src)}</div><div><p class="eyebrow">Output: <code>dctx pack .</code></p>{term(transcript(pack), "kiosk")}</div></div></section>
<section class="sec" aria-labelledby="stamped-h"><h2 id="stamped-h">stamped.dctx_template</h2>
<div class="cols cols-2"><div>{source_block("stamped.dctx_template", stamped_src)}</div><div><p class="eyebrow">Output: <code>dctx pack . --tree-stamps=on</code></p>{term(transcript(stamped), "kiosk")}</div></div></section>
<section class="sec prose" aria-labelledby="syntax-h"><h2 id="syntax-h">Syntax</h2>
<p>From <a href="/docs/templating/">ADR-003</a>, sections 1, 3, 4 and 5. The contract of the source and the compiled plan is <a href="/docs/templates-contract/">DTS v2</a>.</p>
{guide_html}</section>
</div>"""
    parts = []
    for name, text in (("default.dctx_template", default_src), ("stamped.dctx_template", stamped_src)):
        node = {"@type": "SoftwareSourceCode", "@id": ORIGIN + "/templates/#" + name.split(".")[0] + "-h", "name": name,
                "programmingLanguage": "DTS (dctx template)", "codeSampleType": "full", "codeRepository": BLOB.rstrip("/"),
                "url": f"{BLOB}{TEMPLATE_DIR}/{name}", "encodingFormat": "text/plain"}
        if len(text.encode()) <= 2048:
            node["text"] = text
        parts.append(node)
    return body, [{"@merge": True, "hasPart": [{"@id": p["@id"]} for p in parts]}] + parts
