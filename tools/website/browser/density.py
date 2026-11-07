import sys

from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b = p.chromium.launch(); c = b.new_context(viewport={"width": 1440, "height": 900}, reduced_motion="reduce"); pg = c.new_page()
    pg.goto((sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:9190") + "/"); pg.wait_for_timeout(1500)
    print(pg.evaluate("""() => { const H = innerHeight, vis = el => { const r = el.getBoundingClientRect(); return r.top >= 0 && r.bottom <= H && r.height > 0; };
      const inBox = el => { const b = el.closest('.scroll'); if (!b) return true; const r = el.getBoundingClientRect(), q = b.getBoundingClientRect(); return r.top >= q.top && r.bottom <= q.bottom; };
      const count = s => [...document.querySelectorAll(s)].filter(e => vis(e) && inBox(e)).length;
      const geo = JSON.parse(document.getElementById('scene-data').textContent);
      return {chips: count('.chips li'), steps: count('.steps li'), facts: count('.facts > div'), paths: count('.paths > li'), notice: count('.callout'), terminal_lines: count('.term-hero .ln'), nav: count('.nav li'),
              scene_labels: geo.nodes.filter(n => n.id.startsWith('m:')).length + geo.nbrs.length + 3}; }"""))
    b.close()
