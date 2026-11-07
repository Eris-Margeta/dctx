"""Headless load measurement: requests, transferred bytes, FCP, LCP, CLS, console errors."""
import json
import sys

from playwright.sync_api import sync_playwright

INIT = """
window.__cls = 0; window.__lcp = 0;
new PerformanceObserver(l => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({type: 'layout-shift', buffered: true});
new PerformanceObserver(l => { const es = l.getEntries(); window.__lcp = es[es.length - 1].startTime; }).observe({type: 'largest-contentful-paint', buffered: true});
"""


def measure(url, width=1440, height=900, js=True, reduced=False, wait=2500):
    with sync_playwright() as p:
        b = p.chromium.launch()
        ctx = b.new_context(viewport={"width": width, "height": height}, java_script_enabled=js,
                            reduced_motion="reduce" if reduced else "no-preference")
        page = ctx.new_page()
        errors, reqs = [], []
        page.on("console", lambda m: m.type in ("error", "warning") and errors.append(m.text))
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.on("requestfinished", lambda r: reqs.append(r))
        page.on("requestfailed", lambda r: errors.append("failed " + r.url))
        cdp = ctx.new_cdp_session(page)
        cdp.send("Network.enable")
        cdp.send("Network.setCacheDisabled", {"cacheDisabled": True})
        sizes = {}
        cdp.on("Network.loadingFinished", lambda ev: sizes.__setitem__(ev["requestId"], ev["encodedDataLength"]))
        if js:
            page.add_init_script(INIT)
        page.goto(url, wait_until="load")
        page.wait_for_timeout(wait)
        out = {"url": url, "requests": len(reqs), "transfer_bytes": sum(sizes.values()), "errors": errors}
        if js:
            out.update(page.evaluate("""() => { const p = performance.getEntriesByType('paint').find(e => e.name === 'first-contentful-paint');
              const n = performance.getEntriesByType('navigation')[0];
              return {fcp_ms: p ? Math.round(p.startTime) : null, lcp_ms: Math.round(window.__lcp), cls: +window.__cls.toFixed(4),
                      dom_ms: Math.round(n.domContentLoadedEventEnd), load_ms: Math.round(n.loadEventEnd)}; }"""))
        types = {}
        for r in reqs:
            types[r.resource_type] = types.get(r.resource_type, 0) + 1
        out["by_type"] = types
        b.close()
        return out


if __name__ == "__main__":
    for u in sys.argv[1:]:
        print(json.dumps(measure(u)))
