"""Five explainer animations, and the /how-it-works/ page that shows them.

Each public function returns one self-contained <figure data-anim> block: the canvas stage, a step line
with pause and replay buttons, a visible caption with the one takeaway, and the prose fallback. The prose
is shown when JavaScript or motion is off and is read by screen readers when the animation plays (the
canvas is aria-hidden). Drawing is src/anim.js, shipped in the home.js and how.js bundles (assets.BUNDLES):
a page embeds these figures and loads one of those bundles as its single script. Styles are in site.css.

Honesty: anything not in the current release (dctx doctor, binary signing, receipts, the catalog and
shards, prefix tiers) is labelled planned, in the canvas and in the prose; the cost chart is labelled
an illustration, not a measurement. The safe-path commands exist in the current release.
"""
from __future__ import annotations

from common import VERSION, e, rec

# Every animation in page order: (function name, anchor id, heading on /how-it-works/).
ORDER = [("two_scopes", "two-scopes", "Two scopes, one binary"),
         ("calls_times_context", "calls-times-context", "Cost is calls times context"),
         ("prefix_stability", "prefix-stability", "Edit a file, keep the cache"),
         ("giant_folder", "giant-folder", "The giant folder"),
         ("safe_path", "safe-path", "The safe path")]


def _figure(key: str, label: str, takeaway: str, note: str, intro: str, frames: list[str], data: dict | None = None) -> str:
    attrs = "".join(f' data-{k}="{e(v)}"' for k, v in (data or {}).items())
    steps = "".join(f"<li>{f}</li>" for f in frames)
    return (f'<figure class="anim" data-anim="{key}"{attrs}>'
            f'<div class="anim-view"><div class="anim-stage"><canvas aria-hidden="true"></canvas></div>'
            f'<div class="anim-bar"><span class="anim-step" data-step aria-hidden="true"></span>'
            f'<span class="anim-ctl" data-ctl hidden>'
            f'<button class="tbtn" type="button" data-pause aria-pressed="false"><span data-l>Pause</span>'
            f'<span class="visually-hidden"> animation: {e(label)}</span></button>'
            f'<button class="tbtn" type="button" data-replay>Replay<span class="visually-hidden"> animation: {e(label)}</span></button>'
            f'</span></div></div>'
            f'<figcaption><b>{takeaway}</b> {note}</figcaption>'
            f'<div class="anim-prose"><p>{intro}</p><ol>{steps}</ol></div></figure>')


def two_scopes() -> str:
    return _figure(
        "two_scopes", "two scopes, one binary", "Same engine, different owner.",
        "The system <code>dctx</code> is yours; a project's <code>dctx</code> is pinned by that project. "
        "<code>dctx doctor</code> and binary signing are planned.",
        "What the animation shows, step by step:",
        ["One machine.",
         f"The system <code>dctx</code> on <code>PATH</code> (version {VERSION}) belongs to you and your agents; you decide when to upgrade it.",
         "A project vendors its own <code>dctx</code> inside <code>node_modules</code> or a virtualenv, pinned by its lockfile "
         "and upgraded only when the project bumps it. The pip and npm packages are publishing soon.",
         "Both are the same engine at different versions.",
         "Interactive agents and MCP call the system binary; application code and CI call the project binary. "
         "A library never falls back to <code>PATH</code> silently.",
         "<code>dctx doctor</code>, planned and not in the current release, would list every <code>dctx</code> visible from the "
         "current folder with its version and scope. Until binary signing ships, verify prebuilt archives against <code>SHA256SUMS</code>."],
        {"v": VERSION, "pv": "1.1.0"})


def calls_times_context() -> str:
    return _figure(
        "calls_times_context", "cost is calls times context", "Calls cost more than output size.",
        "An illustration, not a measured result: every call re-reads the whole conversation so far.",
        "What the illustration shows (made-up units, not a benchmark):",
        ["Each model call reads the whole conversation: the task and prompt, plus every earlier tool result.",
         "Agent A uses grep and file reads: eight calls with small results. The conversation grows a little with each call, "
         "and each call reads all of it again.",
         "Agent B uses dctx: two calls that return larger, bounded pages, then it is done.",
         "Adding up what every call re-reads (the shaded areas): 164 units for agent A, 29 for agent B.",
         "So the number of calls matters more than the size of one result. The reverse also holds: dctx used without a plan "
         "can add calls instead of removing them, so count the calls of your own sessions. "
         '<a href="/docs/modalities/">Choosing how to use dctx</a> explains how to count them.'])


def prefix_stability() -> str:
    return _figure(
        "prefix_stability", "edit a file, keep the cache", "Edit a file, keep the cache.",
        "Illustrative sizes. The pack keeps a stable order and carries no timestamps by default, so a prompt cache can reuse "
        "every byte before the edit.",
        "What the animation shows:",
        ["Turn 1: the pack is sent as one strip of bytes, file after file in a fixed order; the model provider caches it.",
         "You edit one file near the end of that order.",
         "Turn 2: the same tree gives the same bytes up to that file, so only the edited file and what follows are new.",
         "The cache reuses the unchanged prefix, here about three quarters of the bytes.",
         "If the pack began with a timestamp, the first byte would change and nothing would be reused. dctx answers carry no "
         "timestamps, digests or ages by default; <code>--meta</code> adds them back.",
         "Ordering rarely changed files first (prefix tiers) is planned."])


def giant_folder() -> str:
    return _figure(
        "giant_folder", "the giant folder", "Index what the task touches, never copy the tree.",
        "The catalog and shards are the planned architecture. Today, point dctx at a sub-folder named in the task, "
        "not at the whole tree.",
        "What the animation shows (the planned architecture):",
        ["A 200 GB folder: photos, datasets, archives and a folder of repositories.",
         "A catalog (planned) sweeps it once and records names and sizes only; no file contents are read.",
         "A task arrives: fix invoice rounding.",
         "Two shards match the task: <code>repos/billing</code> and <code>repos/shared-ui</code>.",
         "Only those two get symbol indexes; the rest of the tree is never indexed or copied.",
         "A bounded page, inside the budget and in a stable order, comes out of the two shards.",
         "What works today: start dctx on the sub-folder the task names (for example <code>dctx mcp ~/data/repos/billing</code>), "
         "with ignore rules for the rest."])


def safe_path() -> str:
    pack = rec("pack-for")
    end = [line for line in pack["stdout"].split("\n") if line.startswith("# end ")][-1]
    argv = pack["argv"]
    query, budget = argv[argv.index("--for") + 1], argv[argv.index("--budget") + 1]
    return _figure(
        "safe_path", "the safe path", "Measure before you change anything.",
        f"The safe path for a new user, with commands that exist in dctx {VERSION}. Receipts are planned.",
        "If you are new, follow the safe path: one project, the system binary, the lean MCP profile. "
        "Measure before you change anything. Step by step:",
        ["A new user with one agent and one repository.",
         'Install the system binary: download the archive for your platform from GitHub Releases, check it with '
         '<code>sha256sum -c SHA256SUMS --ignore-missing</code>, then run <code>dctx --version</code>.',
         "Pick one project, not your home folder or a huge tree. The system binary serves it; nothing is installed per project.",
         "Connect the lean MCP profile: <code>dctx mcp --profile lean /abs/path/to/app</code> exposes two tools, find and read, "
         "with compact descriptions, so fewer tool definitions ride along in every turn.",
         f"Make the first call, for example <code>dctx pack . --for &#x27;{e(query)}&#x27; --budget {budget}</code>, and read its last "
         f"line: <code>{e(end)}</code> on the sample project.",
         "Measure: per task, count the calls, the start-up tokens and the total input, and compare with grep and file reads "
         "on the same task. Receipts that count this for you are planned.",
         "Only then change one thing at a time."],
        {"v": VERSION, "q": query, "b": budget, "end": end})


INTRO = {
    "two_scopes": "The same engine can live in two places on one machine. Knowing which one you are calling avoids most version confusion.",
    "calls_times_context": "Whole-session cost depends on how many times the model reads the conversation, not only on how big one answer is.",
    "prefix_stability": "Prompt caches reuse bytes up to the first change. A stable order without timestamps keeps that prefix long.",
    "giant_folder": "For a very large folder, the plan is to catalog names once and index only the parts a task needs.",
    "safe_path": "The shortest way to find out whether dctx helps your work, without changing several things at once.",
}


def page() -> str:
    """/how-it-works/: all five animations with a short introduction each."""
    toc = "".join(f'<li><a href="#{a}">{e(h)}</a></li>' for _, a, h in ORDER)
    secs = "".join(
        f'<section class="sec" aria-labelledby="{a}"><h2 id="{a}">{e(h)}</h2><p>{e(INTRO[fn])}</p>{globals()[fn]()}</section>'
        for fn, a, h in ORDER)
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Docs</span><h1>How dctx works</h1>
<p class="lede">Five short animations. Each has a caption with its one point and a text version underneath it when motion is off.
Use the Motion toggle in the header to switch between them. Planned features are marked planned.</p></div>
<ul class="how-list" aria-label="On this page">{toc}</ul>
{secs}
<p>Next: <a href="/docs/modalities/">Choosing how to use dctx</a> &middot; <a href="/examples/">Examples</a> &middot; <a href="/benchmarks/">Benchmarks</a></p></div>"""
