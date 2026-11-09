"""The home hero visual: one real run on the sample project, from folder to context page, in four steps.

Everything shown is captured data: the file statuses and the graph come from og.scene_geometry() (the
same data as the OG images), the ranked symbols and the page from the `pack --for` record in demo.json.
The final state is plain HTML and inline SVG, so it reads without JS and with motion off; hero.js only
plays the steps in order (CSS transitions) when motion is on."""
from __future__ import annotations

import re

import highlight
from common import e, rec
from components import command

QUERY = "pack-for"
STATUS = {  # scene status -> (tag shown, class, what policy did)
    "packed": ("read", "ok", "read"),
    "listed-not-read": ("name only", "warn", "listed, never read"),
    "binary": ("binary", "skip", "not listed"),
    "ignored": ("ignored", "skip", "not listed"),
}


def _files(scene: dict) -> str:
    rows = []
    for i, f in enumerate(scene["files"]):
        tag, cls, why = STATUS[f["s"]]
        path = f["p"]
        head, _, name = path.rpartition("/")
        shown = (f'<span class="dir">{e(head)}/</span>' if head else "") + e(name)
        rows.append(f'<li class="f-{cls}" data-i="{i}"><span class="fp">{shown}</span>'
                    f'<span class="tag" title="{e(why)}">{e(tag)}</span></li>')
    return '<ul class="files" aria-label="Files of the sample project and what the policy did with each">' + "".join(rows) + "</ul>"


def _graph(scene: dict, found: set[str], near: set[str]) -> str:
    """The real node graph, y squeezed so it fits a wide panel; found symbols and their neighbours lit."""
    nodes = scene["nodes"]
    xs = [n["x"] for n in nodes]
    ys = [n["y"] * .5 for n in nodes]
    x0, y0 = min(xs) - 14, min(ys) - 14
    w, h = max(xs) - x0 + 14, max(ys) - y0 + 14
    pt = lambda i: (round(nodes[i]["x"] - x0, 1), round(nodes[i]["y"] * .5 - y0, 1))  # noqa: E731
    lines = []
    for a, b in scene["tree"]:
        (ax, ay), (bx, by) = pt(a), pt(b)
        lines.append(f'<line class="et" x1="{ax}" y1="{ay}" x2="{bx}" y2="{by}"/>')
    for a, b, _kind in scene["edges"]:
        (ax, ay), (bx, by) = pt(a), pt(b)
        lit = " lit" if nodes[a]["id"] in found | near and nodes[b]["id"] in found | near else ""
        lines.append(f'<line class="ec{lit}" x1="{ax}" y1="{ay}" x2="{bx}" y2="{by}"/>')
    dots = []
    for i, n in enumerate(nodes):
        x, y = pt(i)
        cls = "hit" if n["id"] in found else "nb" if n["id"] in near else "mod" if n["id"].startswith("m:") else ""
        r = 9 if cls == "hit" else 7 if n["id"].startswith("m:") else 5
        dots.append(f'<circle class="{cls}" cx="{x}" cy="{y}" r="{r}"/>' if cls else f'<circle cx="{x}" cy="{y}" r="{r}"/>')
    return (f'<svg class="graph" viewBox="0 0 {round(w)} {round(h)}" role="img" aria-label="The symbol graph of the sample project: '
            f'{len(nodes)} nodes; the matches and their neighbours are highlighted">{"".join(lines)}{"".join(dots)}</svg>')


def _ranked(out: str) -> tuple[str, set[str], set[str]]:
    """The find, neighbors and read sections of the pack --for page, as short rows."""
    rows, found, near, sec = [], set(), set(), ""
    for line in out.split("\n"):
        if line.startswith("## "):
            sec = line[3:]
            continue
        if line.startswith("#") or not sec:
            continue
        if sec.startswith("find "):
            sid, kind, span = line.split("\t")
            found.add(sid)
            path, _, name = sid[2:].partition("#")
            rows.append(("find", sec[5:], kind, name, f"{path}:{span[1:]}"))
        elif sec.startswith("neighbors "):
            m = re.match(r"^(\w+):(in|out):([^\t]+)\t(\w+)", line)
            if m and m.group(1) != "contains":
                near.add(m.group(3))
                other = m.group(3).rpartition("#")[2]
                me = sec[10:].rpartition("#")[2]
                a, b = (other, me) if m.group(2) == "in" else (me, other)
                rows.append(("near", m.group(1), m.group(4), f"{a} → {b}", m.group(1)))
            elif m:
                near.add(m.group(3))
        elif sec.startswith("read ") and "\t" in line:
            rows.append(("read", "", line.split("\t")[1], line.split("\t")[0].rpartition("#")[2], "source in the page"))
    html = []
    for kind, term, k, name, where in rows:
        label = {"find": f"find {term}", "near": "neighbor", "read": "read"}[kind]
        html.append(f'<li class="r-{kind}"><span class="rk">{e(label)}</span><span class="rn"><i>{e(k)}</i> {e(name)}</span>'
                    f'<span class="rw">{e(where)}</span></li>')
    return '<ol class="ranked" aria-label="What the query selected, in page order">' + "".join(html) + "</ol>", found, near


def flow(scene: dict) -> str:
    c = scene["counts"]
    r = rec(QUERY)
    out = r["stdout"].rstrip("\n")
    ranked, found, near = _ranked(out)
    page = "".join(f'<span class="pl">{ln or " "}</span>' for ln in highlight.lines(out, "map"))
    terms = r["argv"][r["argv"].index("--for") + 1]
    return f"""<div class="flow" data-flow>
 <div class="flow-head">
  <h2 class="eyebrow" id="scene-h">On the sample project <a href="/examples/">kiosk</a>: one command, four steps</h2>
  <p class="flow-cmd"><code><span class="p">$ </span>{highlight.highlight(command(r), "sh")}</code>
   <button class="tbtn" type="button" data-replay hidden>Replay</button></p>
 </div>
 <ol class="flow-steps">
  <li class="fs fs-repo" data-step="1">
   <div class="fs-head"><span class="n">1</span><b>repository</b><span class="n">2</span><b>policy</b></div>
   {_files(scene)}
   <p class="fs-foot"><b>{c["disk"]}</b> files on disk <span class="arr" aria-hidden="true">→</span> <b>{c["listed"]}</b> listed, <b>{c["read"]}</b> read</p>
  </li>
  <li class="fs fs-graph" data-step="3">
   <div class="fs-head"><span class="n">3</span><b>graph</b><span class="q">--for <q>{e(terms)}</q></span></div>
   {_graph(scene, found, near)}
   {ranked}
   <p class="fs-foot"><b>{c["nodes"]}</b> nodes, <b>{c["edges"]}</b> call/import edges</p>
  </li>
  <li class="fs fs-page" data-step="4">
   <div class="fs-head"><span class="n">4</span><b>bounded page</b><span class="q">for the model</span></div>
   <pre class="page" tabindex="0" aria-label="The page dctx printed, in full">{page}</pre>
   <svg class="meter" viewBox="0 0 {c["budget"]} 8" preserveAspectRatio="none" role="img" aria-label="{c["body_tokens"]} of {c["budget"]} tokens used, {c["dropped"]} dropped"><rect width="{c["budget"]}" height="8"/><rect class="used" width="{c["body_tokens"]}" height="8"/></svg>
   <p class="fs-foot"><b>{c["body_tokens"]}</b> of {c["budget"]} tokens, <b>{c["dropped"]}</b> dropped, same order every turn</p>
  </li>
 </ol>
</div>"""
