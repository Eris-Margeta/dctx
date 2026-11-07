"""News posts. Moved unchanged from the previous site generator; repository links now point to
the rendered documents on this site."""
from __future__ import annotations

from common import VERSION

NEWS = [
    {
        "slug": "dctx-1-1", "date": "2026-11-04", "kind": "Release",
        "title": "dctx 1.1: three more languages, a prefix-stable pack, and a measured lead.",
        "summary": "Perl, Fortran and Verilog join map mode; dctx pack drops timestamps so prompt caches hit; the three-way benchmark and the byte-stability measurement are public.",
        "body": f"""
<p>dctx {VERSION} was released on 4 November 2026. It folds in the 1.0.1 line and follows the first v1 release of 31 October.</p>
<h2>What is new</h2>
<ul>
<li><strong>Perl, Fortran and Verilog/SystemVerilog</strong> join the full tier, with symbols and lexical call edges labelled <code>guess</code>. dctx now covers 50 language families: 36 full, 8 outline and 6 documentation formats.</li>
<li><strong>A prefix-stable <code>dctx pack</code>.</strong> Tree lines carry the file id only, with no modification time and no size, so a bare <code>touch</code> leaves the page byte-identical and an edit keeps everything before the edited file as a shared prefix. <code>--tree-stamps=on</code> brings back the 1.0.0 lines.</li>
<li><strong>Faster id lookup</strong>, record and entry caps checked during the scan, and a closed stdout pipe that exits cleanly.</li>
</ul>
<h2>Historical output-only measurements</h2>
<p>These other-tool results are retained as release history. Tokens here estimate tool stdout, not whole-session cost. The current baseline is <a href="/benchmarks/#grep-read">grep plus file reads</a>; <a href="/docs/modalities/">choose a mode</a> and count startup context and all model turns.</p>
<p>On our own 70-question protocol, run on 31 October against Trail-Code 0.28.0 and graphify 0.9.97 in code-only mode, dctx scored 140 of 140 points with 29,340 estimated tokens; Trail-Code scored 122 with 88,923 and graphify 96 with 197,203. Cold indexing of the six corpora took dctx 0.13&ndash;1.20 s.</p>
<p>That protocol was designed by the dctx team and can favour dctx. An independently selected protocol is pending, and we will publish its result whatever it shows.</p>
<p>On 6 November, 33 of 33 text answers and 33 of 33 JSON answers stayed byte-identical after unrelated edits across three pinned corpora. Three weeks earlier, answer headers that changed after unrelated edits were one of the findings that <a href="/news/specification-and-benchmark/">our first benchmark</a> held against us.</p>
<p><a href="/docs/benchmark/">Benchmark write-up</a> &middot; <a href="/docs/stability/">Stability report</a> &middot; <a href="/changelog/#v1-1-0">Full changelog</a></p>
"""},
    {
        "slug": "desktop-free-forever", "date": "2026-10-21", "kind": "Announcement",
        "title": "dctx Desktop is now free. Forever.",
        "summary": "For individuals, teams and companies. No trial, no seat count, no expiry.",
        "body": f"""
<p>Starting today, the dctx desktop app is free for everyone who uses it: a single developer, a ten-person studio or an engineering organisation of thousands. It stays that way.</p>
<h2>Why we are doing this</h2>
<p>dctx exists because of one belief: prompt-processing efficiency. Every token in front of a model is paid for three times, in prefill, in cache misses and in attention. The smaller, more exact and more stable the context, the better and cheaper every answer becomes.</p>
<p>The desktop app is how people who do not live in a terminal reach that engine: designers, analysts, product managers and engineers who simply prefer a window. Charging for it put a toll at the front door of the very habit we want to spread. So we are removing the toll.</p>
<blockquote>Efficient context should be the default for everyone who works with a model, not a premium feature.</blockquote>
<h2>What you get</h2>
<ul>
<li>The same engine as the command line and the library: one core, the same privacy guarantee, the same output.</li>
<li>Drag-and-drop or native folder selection, and snapshots generated without blocking the interface.</li>
<li>Your global dctx settings and ignore rules, applied exactly as on the command line.</li>
<li>Everything runs on your machine. No account, no telemetry, no network.</li>
</ul>
<p>The source is in the open repository under Apache-2.0, next to the CLI and the core. The desktop app is a preview on its own version line (0.11), source only: there is no download yet, so build it from the repository.</p>
<h2>What does not change</h2>
<p>dctx remains open source. The repository is ongoing work, and we publish what we measure, including where we still have work to do, as in <a href="/news/specification-and-benchmark/">our first public benchmark</a>.</p>
<p>Organisations that depend on dctx can sign a <a href="/maintenance/">maintenance agreement</a> for security response times, long-term support and priority work. Paying never changes the license, and it funds the work that keeps the desktop app free.</p>
<p><em>Already bought a license? Thank you. You helped build what everyone now gets for free.</em></p>
"""},
    {
        "slug": "specification-and-benchmark", "date": "2026-10-21", "kind": "Engineering",
        "title": "Our specification, and a benchmark that does not flatter us.",
        "summary": "The dctx Context Management Specification, and an isolated comparison with Trail-Code: wins, losses and what comes next.",
        "body": f"""
<div class="caveat"><p><strong>Historical post.</strong> These results describe dctx as of October 2026, before call edges and byte-stable answers. For current measurements, read <a href="/news/dctx-1-1/">dctx 1.1</a>.</p></div>
<p>An engineering organisation should be able to check every claim a tool makes. So we wrote down exactly what dctx is meant to be, and then measured where it stands today.</p>
<h2>The specification</h2>
<p>The dctx Context Management Specification turns our core belief, prompt-processing efficiency, into five testable constraints: every answer is bounded, deterministic, prefix-stable, non-redundant and honest about its precision and freshness. It names ten ways chunk retrieval fails and the mechanism that rules out each one, defines a seven-layer organisation policy in which every layer can only narrow the one above it, and lays out the roadmap with an acceptance gate for every step. Each item is marked as available, under contract or a target.</p>
<h2>The benchmark</h2>
<p>This is a secondary historical comparison of tool outputs, not whole-session agent cost. The current baseline is <a href="/benchmarks/#grep-read">grep plus file reads</a>.</p>
<p>We ran dctx and Trail-Code side by side on a dedicated machine: seven pinned code bases in Rust, Go, Python and TypeScript, no network during the runs, every command pinned to the same CPUs, and an adversarial corpus with planted secrets. The full report, raw data and scripts are public so anyone can rerun it.</p>
<h3>Where dctx led</h3>
<ul>
<li>Cold indexing on every code base: 0.09&ndash;6.05 s against 2.4&ndash;95 s, with 1.6&ndash;27&times; less memory.</li>
<li>Footprint: one 9.2 MB binary against 405.6 MB of Node.js packages.</li>
<li>Bounds: map answers never exceeded their token budget.</li>
<li>Network: dctx opened no sockets and wrote nothing into the repositories.</li>
</ul>
<h3>Where Trail-Code led</h3>
<ul>
<li>Call graphs: dctx's map command did not yet expose call or import edges.</li>
<li>Answers to agent questions: 122 of 140 points against 69 of 140, with fewer tokens.</li>
<li>Stability under edits: Trail-Code kept most answers byte-identical after an unrelated change; dctx did not, which costs prompt-cache hits.</li>
<li>Incremental refresh after an edit.</li>
</ul>
<h2>What happens next</h2>
<p>The findings are now the first items on our roadmap: call and import edges with honest confidence labels, a persistent incremental index, answer headers that stay stable across unrelated edits, a bounded repository overview, a search verb, and safer defaults for secrets when packing a whole project. Each one ships only when it passes the gate the specification sets for it, and we will publish the next measurement the same way.</p>
<p><a href="/docs/spec/">Read the specification</a> &middot; <a href="/docs/benchmark-oct/">Read the benchmark report</a></p>
"""},
]
