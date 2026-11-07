import sys
import time
BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9190"
from playwright.sync_api import sync_playwright
INIT = "window.__cls=0;new PerformanceObserver(l=>{for(const e of l.getEntries())if(!e.hadRecentInput)window.__cls+=e.value}).observe({type:'layout-shift',buffered:true});"
with sync_playwright() as p:
    b = p.chromium.launch()
    for url in ["/", "/docs/spec/", "/cli/"]:
        for w in (1440, 390):
            c = b.new_context(viewport={"width": w, "height": 900})
            pg = c.new_page(); pg.add_init_script(INIT)
            def slow(route):
                time.sleep(0.8); route.continue_()
            pg.route("**/*.woff2", slow)
            pg.goto(BASE + url); pg.wait_for_timeout(2500)
            print(url, w, "CLS", round(pg.evaluate("window.__cls"), 4))
            c.close()
    b.close()
