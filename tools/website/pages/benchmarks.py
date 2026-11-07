"""/benchmarks/: tables parsed from the benchmark write-up, the run 7 data and the stability report."""
from __future__ import annotations

import json

import data
from common import BLOB, ORG_ID, ORIGIN, TREE, e, line_of, read, src
from components import table
from md import Linker, to_html

RAW = "docs/research/bench-3way-20261030/"


def benchmarks() -> tuple[str, list]:
    r7 = data.run7()
    qline, qhead, qrows = data.table_after(data.BENCH, "Latest: run 5", "By question type, out of 14:")
    cline, chead, crows = data.table_after(data.BENCH, "Cold index (median wall s")
    wline, whead, wrows = data.table_after(data.BENCH, "Warm latency and answer size")
    stab = data.stability()
    agg = json.loads(read(data.RUN7))["index"]
    cold_rows = []
    for corpus in data.CORPORA:
        row = [corpus]
        for tool in ("dctx", "trail", "graphify"):
            cell = agg.get(corpus, {}).get(tool, {})
            value = cell.get("cold_wall_s_median") if cell.get("ok", True) else None
            row.append(f"{value:.2f} s" if value is not None else "no index")
        cold_rows.append(row)
    notclaimed = data.section(read(data.BENCH), "Not claimed")
    notclaimed_html, _, _ = to_html(notclaimed, data.BENCH, Linker({}))
    ind, ind_line, _ = data.independent()
    ind_html, _, _ = to_html(ind, data.BENCH, Linker({}))
    lo, hi, n = r7["cold"]["dctx"]
    tl, th, _ = r7["cold"]["trail"]
    gl, gh, gn = r7["cold"]["graphify"]
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Evidence</span><h1>Benchmarks</h1>
<p class="lede">Compare dctx with grep plus file reads, then measure the whole agent session. Startup instructions, tool definitions and extra turns can outweigh smaller answers.</p></div>
<h2 id="grep-read">Grep plus file reads</h2>
<p>The headline baseline is grep plus reading files. A controlled benchmark against it, counting whole agent sessions (start-up context, every call, provider cache hits and accuracy), is being prepared and will be published with raw data. <a href="/docs/modalities/">Choosing how to use dctx</a> covers scope, cache limits and measured MCP definition bytes.</p>
<h2 id="history">Secondary historical evidence</h2>
<div class="notice"><p>All other-tool tables below preserve the October research record. Their tokens are tool stdout estimates, not whole-session cost; scores measure gold facts in raw output without an agent model. The dctx team designed the 70-question protocol. The independent run selected repositories by seed and used language-server gold.</p></div>
<h2 id="run7">Historical run 7: task-level protocol</h2>
{table(r7["head"], r7["rows"], numeric={1, 2, 3, 4}, caption=f'70 questions, 354 gold facts, 31 October 2026. {src(data.BENCH, "BENCHMARK-20261030.md", r7["line"])}')}
<h2 id="question-types">By question type</h2>
<p>Points out of 14 per type, from run 5 and task re-run 3 (dctx <code>a906884</code>), the last run with a per-type table; run 7 then scored every point on all ten types. {src(data.BENCH, "run 7 note", line_of(data.BENCH, "dctx scores every point on all ten question types"))}</p>
{table(qhead, qrows, numeric={1, 2, 3}, caption=src(data.BENCH, "BENCHMARK-20261030.md", qline))}
<h2 id="independent">Independent benchmark, 31 October 2026</h2>
<div class="prose">{ind_html}</div>
<p class="note">{src(data.BENCH, "BENCHMARK-20261030.md", ind_line)}</p>
<h2 id="cold-index">Cold index</h2>
<div class="cols cols-2"><div>
<p>Run 7 medians of three runs: dctx {lo:.2f}&ndash;{hi:.2f} s on {n} corpora, Trail-Code {tl:.2f}&ndash;{th:.2f} s, graphify {gl:.2f}&ndash;{gh:.2f} s ({gn} corpora; no index on docs).</p>
{table(["corpus", "dctx", "Trail-Code", "graphify"], cold_rows, numeric={1, 2, 3}, caption=src(data.RUN7, "run7 aggregate.json"))}
</div><div>
<p>Run 1 (30 October), with peak memory and index size: median wall s, peak RSS of the largest process, index size.</p>
{table(chead, crows, caption=src(data.BENCH, "BENCHMARK-20261030.md", cline))}
</div></div>
<h2 id="warm">Warm queries and answer size</h2>
{table(whead, wrows, caption="Run 1, median ms of 5 after one warm-up, including process start, and estimated tokens. " + src(data.BENCH, "BENCHMARK-20261030.md", wline))}
<h2 id="stability">Byte stability</h2>
{table(stab["head"], stab["rows"], numeric={1, 2}, caption=f'Build <code>0495f5f</code>, 6 November 2026, three pinned corpora, 11 query shapes each. {src(data.STAB, "REPORT.md", stab["line"])} &middot; <a href="/docs/stability/">full report</a>')}
<h2 id="not-claimed">Not claimed</h2>
<div class="prose">{notclaimed_html}</div>
<h2 id="raw">Raw data</h2>
<ul><li><a href="{TREE}{RAW}">{RAW}</a>: questions, gold facts, scorer, raw results and scripts</li>
<li><a href="/docs/benchmark/">The full write-up</a>, with every run and the method</li>
<li><a href="/docs/benchmark-oct/">The earlier two-way study</a> (October 2026, before call edges)</li></ul>
</div>"""
    dataset = {"@type": "Dataset", "@id": ORIGIN + "/benchmarks/#dataset", "name": "dctx three-way benchmark, run 7",
               "description": "Task-level protocol (70 questions, 354 gold facts), cold index and warm query timings for dctx, Trail-Code and graphify on six pinned corpora.",
               "creator": {"@id": ORG_ID}, "license": "https://www.apache.org/licenses/LICENSE-2.0", "isAccessibleForFree": True,
               "measurementTechnique": "70-question task protocol (docs/BENCHMARK-20261030.md)", "temporalCoverage": "2026-10-31",
               "url": ORIGIN + "/benchmarks/",
               "distribution": {"@type": "DataDownload", "encodingFormat": "application/json", "contentUrl": f"{BLOB}{data.RUN7}"}}
    return body, [dataset]
