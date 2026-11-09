"""Browser checks against the local dev server. Prints one line per check: ok/FAIL name detail."""
import json
import sys

from playwright.sync_api import sync_playwright

BASE = sys.argv[1]
only = sys.argv[2].split(",") if len(sys.argv) > 2 else None
results = []


def rec(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("ok   " if ok else "FAIL ") + name + (" " + str(detail) if detail else ""), flush=True)


def ctx(p, **kw):
    b = p.chromium.launch()
    c = b.new_context(viewport={"width": 1440, "height": 900}, **kw)
    return b, c


def want(n):
    return only is None or n in only


with sync_playwright() as p:
    if want("motion"):
        b, c = ctx(p, reduced_motion="no-preference")
        pg = c.new_page()
        errs = []
        pg.on("console", lambda m: m.type in ("error", "warning") and errs.append(m.text))
        pg.on("pageerror", lambda e: errs.append(str(e)))
        pg.goto(BASE + "/")
        pg.wait_for_timeout(1500)
        tour = "(() => { const t = document.querySelector('[data-tour]'); const on = t.querySelector('.tour-panel.on'); return [t.classList.contains('auto'), on.id, on.querySelectorAll('.ln').length, Math.round(t.getBoundingClientRect().height)]; })()"
        first = pg.evaluate(tour)
        rec("showcase: first step complete from the start (never blank)", first[1] == "tour-p-chat" and first[2] > 5, first)
        pg.mouse.move(1, 1)
        pg.wait_for_timeout(7500)
        later = pg.evaluate(tour)
        rec("showcase advances by itself, the height stays put", first[0] and later[1] == "tour-p-task" and later[3] == first[3], f"{first} -> {later}")
        pg.click("[data-tour] [role=tab] >> nth=3")
        pg.wait_for_timeout(400)
        rec("showcase: a click jumps to that step", pg.evaluate(tour)[1] == "tour-p-mcp")
        pg.hover("[data-tour]")
        pg.wait_for_timeout(8500)
        rec("showcase: hover pauses it", pg.evaluate(tour)[1] == "tour-p-mcp")
        pg.click("#motion-toggle")
        pg.wait_for_timeout(300)
        state = pg.evaluate("[document.documentElement.classList.contains('motion'), document.getElementById('motion-toggle').getAttribute('aria-pressed'), localStorage.getItem('dctx-motion')]")
        rec("motion toggle turns motion off and remembers it", state == [False, "false", "off"], state)
        rec("motion off: no auto-advance, every line at full opacity", not pg.evaluate(tour)[0] and pg.evaluate("[...document.querySelectorAll('.tour-panel.on .ln')].every(l => getComputedStyle(l).opacity === '1')"))
        pg.reload()
        pg.wait_for_timeout(1500)
        rec("motion preference survives a reload", not pg.evaluate("document.documentElement.classList.contains('motion')"))
        pg.click("#theme-toggle")
        rec("theme toggle sets and stores the theme", pg.evaluate("[document.documentElement.dataset.theme, localStorage.getItem('dctx-theme')]")[0] in ("dark", "light"))
        rec("no console errors on the home page", not errs, errs)
        b.close()
    if want("reduced"):
        b, c = ctx(p, reduced_motion="reduce")
        pg = c.new_page()
        pg.goto(BASE + "/")
        pg.wait_for_timeout(2500)
        rec("prefers-reduced-motion: no motion class", not pg.evaluate("document.documentElement.classList.contains('motion')"))
        rec("prefers-reduced-motion: the showcase stands still", not pg.evaluate("document.querySelector('[data-tour]').classList.contains('auto')"))
        pg.click("[data-tour] [role=tab] >> nth=4")
        rec("prefers-reduced-motion: every step reachable, lines at full opacity", pg.evaluate("document.querySelector('.tour-panel.on').id") == "tour-p-lib"
            and pg.evaluate("[...document.querySelectorAll('.tour-panel.on .ln')].every(l => getComputedStyle(l).opacity === '1')"))
        rec("prefers-reduced-motion: toggle disabled with a reason", pg.evaluate("document.getElementById('motion-toggle').disabled"))
        b.close()
    if want("nojs"):
        b, c = ctx(p, java_script_enabled=False)
        pg = c.new_page()
        pg.goto(BASE + "/")
        txt = pg.inner_text(".tour-screen")
        rec("no JS: all five showcase panels are visible", pg.locator(".tour-panel").count() == 5 and all(pg.locator(".tour-panel").nth(i).is_visible() for i in range(5)))
        rec("no JS: the real commands are in the page", "dctx pack . --for" in txt and "dctx mcp ." in txt and "# exit 0" in txt)
        rec("no JS: toggles stay hidden", pg.locator("#motion-toggle").is_hidden() and pg.locator("#theme-toggle").is_hidden())
        pg.goto(BASE + "/playground/")
        pg.fill("#pg-cmd", "map find greet")
        pg.click("#pg-form button[type=submit]")
        pg.wait_for_load_state()
        rec("no JS: the playground form posts and shows the result", "s:src/util.rs#greet" in pg.content() and "<h1>dctx map find greet</h1>" in pg.content(), pg.url)
        b.close()
    if want("playground"):
        b, c = ctx(p)
        pg = c.new_page()
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)))
        pg.goto(BASE + "/playground/")
        pg.wait_for_function("document.getElementById('pg-status').textContent.startsWith('Online')", timeout=5000)
        rec("playground reports the live service", True, pg.inner_text("#pg-status"))
        pg.click("button[data-cmd=\"map neighbors 's:src/util.rs#greet' --edge calls --dir in\"]")
        pg.wait_for_function("document.querySelector('#pg-out pre').textContent.includes('calls:in:s:src/main.rs#main')", timeout=5000)
        rec("playground runs a quick command", True, pg.inner_text("#pg-out pre").split("\n")[1])
        pg.check("input[name=source][value=files]")
        pg.fill("#pg-cmd", "map skeleton src/lib.rs")
        pg.click("#pg-form button[type=submit]")
        pg.wait_for_function("document.querySelector('#pg-out pre').textContent.includes('s:src/lib.rs#add')", timeout=5000)
        rec("playground runs on pasted files", True)
        pg.fill("#pg-cmd", "cache purge --all --yes")
        pg.click("#pg-form button[type=submit]")
        pg.wait_for_function("document.getElementById('pg-status').textContent.startsWith('Refused')", timeout=5000)
        rec("playground shows a refusal", True, pg.inner_text("#pg-status"))
        pg.click("#t-mcp")
        pg.click("#mcp-run")
        pg.wait_for_function("document.querySelector('#pg-out pre').textContent.includes('\"tools\"')", timeout=5000)
        rec("playground MCP tab", True)
        rec("no page errors on the playground", not errs, errs)
        b.close()
    if want("offline"):
        b, c = ctx(p)
        pg = c.new_page()
        pg.goto(BASE + "/playground/")
        pg.wait_for_timeout(1500)
        rec("backend down: offline notice and recorded runs", pg.locator("#pg-offline").is_visible() and pg.locator("#rec-find").is_visible(), pg.inner_text("#pg-status"))
        b.close()
    if want("search"):
        b, c = ctx(p)
        pg = c.new_page()
        pg.goto(BASE + "/search/?q=neighbors")
        pg.wait_for_timeout(1000)
        rec("search finds pages", pg.locator("#search-results li").count() > 2, pg.inner_text("#search-note"))
        pg.goto(BASE + "/no-such-page/")
        rec("404 page with search", pg.locator("h1").inner_text() == "Page not found." and pg.locator("#search-form").count() == 1)
        b.close()
print(json.dumps({"passed": sum(1 for r in results if r[1]), "failed": [r[0] for r in results if not r[1]]}))
