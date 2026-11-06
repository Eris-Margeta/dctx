"""/playground/: a form that works without JavaScript, the live API with it, recorded runs always."""
from __future__ import annotations

import json

from common import ORG_ID, ORIGIN, VERSION, demo, e, read, rec
from components import term, transcript

QUICK = ["map find greet", "map find greet --json", "map neighbors 's:src/util.rs#greet' --edge calls --dir in",
         "map read 's:src/util.rs#greet' --json", "map skeleton src/store.rs", "map hubs --limit 10",
         "map path 's:src/main.rs#main' 's:src/store.rs#save'", "map export --format mermaid", "pack . --for 'greet store' --budget 600",
         "pack . --ext rs", "languages"]
SAMPLE_FILES = """--- src/lib.rs ---
/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn twice(x: i32) -> i32 {
    add(x, x)
}
--- app/main.py ---
from lib import twice


def run():
    return twice(21)
"""
MCP_DEFAULT = "\n".join(json.dumps(m, separators=(",", ":")) for m in [
    {"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "find", "arguments": {"query": "greet"}}},
])


def samples_static() -> list[tuple[str, str]]:
    """The bundled samples, from the same file the playground image uses."""
    return [tuple(line.split("\t", 1)) for line in read("tools/playground/samples.tsv").splitlines() if line.strip()]


def playground() -> tuple[str, list]:
    openapi = json.loads(read("tools/playground/openapi.json"))
    opts = "".join(f'<option value="{i}">{e(t)}</option>' for i, t in samples_static())
    quick = "".join(f'<button type="button" data-cmd="{e(q)}">{e(q)}</button>' for q in QUICK)
    recorded = "".join(f'<section class="ex" id="rec-{r}"><div class="ex-head"><h3>{e(" ".join(rec(r)["argv"]))}</h3></div>{term(transcript(rec(r)), "recorded on kiosk")}</section>'
                       for r in ("find", "neighbors-in", "skeleton", "path"))
    paths = "".join(f"<li><code>{e(m.upper())} {e(p)}</code> {e(spec.get('summary', ''))}</li>"
                    for p, ops in openapi["paths"].items() for m, spec in ops.items())
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Playground</span><h1>Playground</h1>
<p class="lede">Run the real <code>dctx {VERSION}</code> on a bundled sample or on text files you paste. Each run gets a fresh directory and an empty environment, and is limited to 3 s, 2 s of CPU and 256 MiB; nothing is stored. Commands are checked against the CLI's own options and never reach a shell.</p></div>
<div class="pg">
 <div>
  <div class="tabs js-only" role="tablist" aria-label="Mode"><button type="button" role="tab" id="t-cmd" aria-controls="p-cmd" aria-selected="true">Command</button><button type="button" role="tab" id="t-mcp" aria-controls="p-mcp" aria-selected="false">MCP</button></div>
  <div id="p-cmd" role="tabpanel" aria-labelledby="t-cmd">
   <form id="pg-form" method="post" action="/api/v1/run-form">
    <div class="field"><label for="pg-cmd">Command (after <code>dctx</code>)</label><input type="text" id="pg-cmd" name="cmd" value="map find greet" autocomplete="off" spellcheck="false" required></div>
    <div class="quick js-only" aria-label="Examples">{quick}</div>
    <fieldset><legend class="visually-hidden">Place</legend><div class="radios">
     <label><input type="radio" name="source" value="sample" checked> A sample</label>
     <label><input type="radio" name="source" value="files"> My files</label></div></fieldset>
    <div class="field"><label for="pg-sample">Sample</label><select id="pg-sample" name="sample">{opts}</select></div>
    <div class="field"><label for="pg-files">My files: each starts with a line <code>--- path ---</code>; text only, at most 48 files, 96 KiB each</label><textarea id="pg-files" name="files" spellcheck="false">{e(SAMPLE_FILES)}</textarea></div>
    <div class="actions"><button class="btn btn-primary" type="submit">Run</button><a class="btn" href="/api/v1/openapi.json">API (OpenAPI)</a></div>
   </form>
  </div>
  <div id="p-mcp" role="tabpanel" aria-labelledby="t-mcp" hidden>
   <div class="field"><label for="mcp-in">JSON-RPC messages for <code>dctx mcp</code>, one per line (at most 4); the place is the sample or files chosen on the Command tab</label><textarea id="mcp-in" spellcheck="false">{e(MCP_DEFAULT)}</textarea></div>
   <div class="actions"><button class="btn btn-primary" type="button" id="mcp-run">Send</button></div>
  </div>
  <p class="status" id="pg-status" role="status" aria-live="polite"><span class="no-js-only">Without JavaScript the form opens the result as a page.</span></p>
  <div class="notice" id="pg-offline" hidden><p>The playground service is not reachable. Everything else on this site works, and the runs below were recorded from dctx {VERSION} when the site was built.</p></div>
 </div>
 <div>
  {term('<span class="ln out hdrl"># results appear here</span>', "dctx", cls="pg-out").replace('class="term pg-out"', 'class="term pg-out" id="pg-out"')}
  <h2 class="eyebrow" id="api-h">API</h2>
  <ul class="small">{paths}</ul>
 </div>
</div>
<h2 id="recorded">Recorded runs</h2>
<p>Captured at build time on the kiosk sample; the playground returns the same bytes for the same command.</p>
{recorded}
</div>"""
    api = {"@type": "WebAPI", "@id": ORIGIN + "/playground/#api", "name": "dctx playground API",
           "description": openapi["info"]["summary"], "documentation": ORIGIN + "/api/v1/openapi.json",
           "url": ORIGIN + "/api/v1/", "provider": {"@id": ORG_ID}, "termsOfService": ORIGIN + "/playground/"}
    return body, [api]


def offline() -> str:
    return f"""<div class="wrap notfound">{{{{crumbs}}}}<span class="eyebrow">503</span><h1>The playground is offline.</h1>
<p class="lede">The service that runs dctx is not reachable right now; the rest of the site works. The examples were recorded from dctx {VERSION} when this site was built.</p>
<div class="actions"><a class="btn btn-primary" href="/examples/">Recorded examples</a><a class="btn" href="/playground/">Playground</a></div></div>"""
