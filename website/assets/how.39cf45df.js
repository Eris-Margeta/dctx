/* Conveniences on every page: theme and motion toggles, copy buttons, sortable tables, menu.
   The page is complete without this file. */
(function () {
  'use strict';
  var root = document.documentElement;
  function $$(s, c) { return [].slice.call((c || document).querySelectorAll(s)); }
  function store(k, v) { try { localStorage.setItem(k, v); } catch (e) {} }

  var tt = document.getElementById('theme-toggle');
  if (tt) {
    tt.hidden = false;
    tt.addEventListener('click', function () {
      var cur = root.getAttribute('data-theme') || (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
      var next = cur === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next); store('dctx-theme', next);
      document.dispatchEvent(new CustomEvent('dctx:theme'));
    });
  }

  var mt = document.getElementById('motion-toggle'), reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  function label() {
    var on = root.classList.contains('motion') && !reduce.matches;
    mt.textContent = 'Animations: ' + (on ? 'on' : 'off');
    mt.setAttribute('aria-pressed', String(on));
    mt.disabled = reduce.matches;
    mt.title = reduce.matches ? 'Your system asks for reduced motion' : 'Turn animations ' + (on ? 'off' : 'on');
  }
  if (mt) {
    mt.hidden = false; label();
    mt.addEventListener('click', function () {
      var on = !root.classList.contains('motion');
      root.classList.toggle('motion', on); store('dctx-motion', on ? 'on' : 'off'); label();
      document.dispatchEvent(new CustomEvent('dctx:motion'));
    });
    if (reduce.addEventListener) reduce.addEventListener('change', function () { root.classList.toggle('motion', !reduce.matches); label(); document.dispatchEvent(new CustomEvent('dctx:motion')); });
  }

  /* Copy buttons: .code blocks carry a hidden button; highlighted blocks and terminals get one. */
  function copyText(block) {
    var pre = block.querySelector('pre'); if (!pre) return '';
    var cmds = $$('.cmd', pre);
    if (block.classList.contains('term') && cmds.length) return cmds.map(function (c) { return c.textContent.replace(/^\$ /, ''); }).join('\n');
    var copy = pre.cloneNode(true); $$('.p', copy).forEach(function (p) { p.remove(); });
    $$('.glyph-wrap', copy).forEach(function (icon) { icon.replaceWith(document.createTextNode(icon.getAttribute('aria-label'))); });
    return copy.textContent.replace(/\n$/, '');
  }
  $$('.hl, .code, .term').forEach(function (block) {
    var btn = block.querySelector('.copy');
    if (!btn) {
      btn = document.createElement('button'); btn.type = 'button'; btn.className = 'copy'; btn.textContent = 'Copy';
      var bar = block.querySelector('.term-bar, .code-head'); (bar || block).appendChild(btn);
    }
    btn.hidden = false;
    btn.addEventListener('click', function () {
      var text = copyText(block), done = function (msg) { btn.textContent = msg; setTimeout(function () { btn.textContent = 'Copy'; }, 1600); };
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(function () { done('Copied'); }, function () { done('Select and copy'); });
      else done('Select and copy');
    });
  });

  /* Sortable tables: click a header with data-sort. */
  $$('table[data-sortable]').forEach(function (table) {
    var heads = $$('th[data-sort]', table), body = table.tBodies[0];
    heads.forEach(function (th, col) {
      th.tabIndex = 0;
      function sort() {
        var asc = th.getAttribute('aria-sort') !== 'ascending';
        heads.forEach(function (h) { h.removeAttribute('aria-sort'); });
        th.setAttribute('aria-sort', asc ? 'ascending' : 'descending');
        var idx = [].indexOf.call(th.parentNode.children, th);
        $$('tr', body).sort(function (a, b) {
          var x = a.children[idx].textContent.trim(), y = b.children[idx].textContent.trim();
          return (asc ? 1 : -1) * x.localeCompare(y, 'en', { numeric: true });
        }).forEach(function (tr) { body.appendChild(tr); });
      }
      th.addEventListener('click', sort);
      th.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); sort(); } });
    });
  });

  /* Choose your path: remember the last persona so the selector can mark it. Optional: every page
     works the same without storage (private windows, blocked site data). */
  var PATH_KEY = 'dctx-path';
  function recall(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  var pathPage = document.querySelector('[data-path-page]');
  if (pathPage) store(PATH_KEY, pathPage.getAttribute('data-path-page'));
  $$('[data-path-select]').forEach(function (list) {
    var chosen = recall(PATH_KEY);
    $$('[data-path]', list).forEach(function (card) {
      var id = card.getAttribute('data-path'), link = card.querySelector('a');
      if (link) link.addEventListener('click', function () { store(PATH_KEY, id); });
      if (id === chosen) {
        card.classList.add('picked');
        var tag = document.createElement('p'); tag.className = 'picked-tag'; tag.textContent = 'Your last choice';
        card.insertBefore(tag, card.firstChild);
      }
    });
  });

  var menu = document.querySelector('.menu');
  if (menu) {
    var menuToggle = menu.querySelector('summary'), menuPanel = menu.querySelector('.menu-panel');
    menuPanel.id = 'mobile-menu';
    menuToggle.setAttribute('aria-controls', menuPanel.id);
    function syncMenu() { menuToggle.setAttribute('aria-expanded', String(menu.open)); }
    syncMenu();
    menu.addEventListener('toggle', syncMenu);
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && menu.open) { menu.open = false; menuToggle.focus(); } });
  }

  /* Scroll-spy: one observer for all section headings, shared by the desktop/mobile TOCs and
     section lists. Page links in the documentation sidebar keep aria-current="page". */
  var sectionNavs = $$('.toc, .toc-m, .toc-row, .how-list'), headings = [];
  var spies = sectionNavs.map(function (nav) {
    var entries = $$('a[href^="#"]', nav).map(function (link) {
      var id;
      try { id = decodeURIComponent(link.hash.slice(1)); } catch (e) { return null; }
      var target = document.getElementById(id);
      if (!target) return null;
      var heading = /^H[1-6]$/.test(target.tagName) ? target : target.querySelector('h1, h2, h3, h4, h5, h6');
      if (!heading) return null;
      if (headings.indexOf(heading) < 0) headings.push(heading);
      return { link: link, heading: heading, target: target };
    }).filter(Boolean);
    entries.sort(function (a, b) { return a.heading.compareDocumentPosition(b.heading) & 2 ? 1 : -1; });
    return { nav: nav, entries: entries };
  });
  function spyUpdate() {
    // Read just below the sticky header; at the foot of the page include even a short last section.
    var line = (parseFloat(getComputedStyle(root).scrollPaddingTop) || 0) + 1;
    var bottom = window.scrollY > 0 && Math.ceil(window.scrollY + window.innerHeight) >= root.scrollHeight - 1;
    spies.forEach(function (spy) {
      var active = spy.entries[0];
      spy.entries.forEach(function (entry) {
        // Some links anchor the section containing the heading (for example, a release).
        if (bottom || entry.target.getBoundingClientRect().top <= line) active = entry;
      });
      spy.entries.forEach(function (entry) {
        if (entry === active) entry.link.setAttribute('aria-current', 'location');
        else entry.link.removeAttribute('aria-current');
      });
      // Adjust only the sidebar's scroll position: scrollIntoView can move the document too.
      if (active && spy.nav.clientHeight && spy.nav.scrollHeight > spy.nav.clientHeight &&
          /auto|scroll/.test(getComputedStyle(spy.nav).overflowY)) {
        var box = spy.nav.getBoundingClientRect(), link = active.link.getBoundingClientRect();
        if (link.top < box.top) spy.nav.scrollTop -= box.top - link.top;
        else if (link.bottom > box.top + spy.nav.clientHeight) spy.nav.scrollTop += link.bottom - box.top - spy.nav.clientHeight;
      }
    });
  }
  if (headings.length) {
    var spyFrame = 0;
    function spySchedule() {
      if (!spyFrame) spyFrame = requestAnimationFrame(function () { spyFrame = 0; spyUpdate(); });
    }
    if ('IntersectionObserver' in window) {
      var spyObserver = new IntersectionObserver(spySchedule, { rootMargin: '0px 0px -70% 0px' });
      headings.forEach(function (heading) { spyObserver.observe(heading); });
    }
    // Scroll also covers long sections, rapid jumps past the observer band and the page's last section.
    window.addEventListener('scroll', spySchedule, { passive: true });
    window.addEventListener('resize', spySchedule);
    window.addEventListener('hashchange', spySchedule);
    window.addEventListener('load', spySchedule);
    window.addEventListener('pageshow', spySchedule);
    spyUpdate();
  }

  /* New version check: each page carries the id of the build it belongs to (meta dctx-build); /build.json
     names the deployed one. On load, when the tab comes back and every 10 minutes, a different digest
     offers a reload. Same origin only, never cached, nothing stored. */
  var meta = document.querySelector('meta[name="dctx-build"]'), mine = meta ? meta.content : '', note = null, HEX = /^[0-9a-f]{64}$/;
  function offer() {
    note = document.createElement('div'); note.className = 'update'; note.setAttribute('role', 'status');
    note.innerHTML = '<p><b>New version available.</b> This page was updated since you opened it.</p>'
      + '<button class="btn btn-primary" type="button">Reload</button><button class="btn" type="button">Dismiss</button>';
    var b = note.querySelectorAll('button');
    b[0].addEventListener('click', function () { location.reload(); });
    b[1].addEventListener('click', function () { note.remove(); });
    document.body.appendChild(note);
  }
  function check() {
    if (note || document.hidden || !window.fetch) return;
    fetch('/build.json', { cache: 'no-store', credentials: 'same-origin' }).then(function (r) { return r.ok ? r.json() : null; })
      .then(function (b) { if (!note && b && HEX.test(b.build_id || '') && b.build_id !== mine) offer(); }, function () {});
  }
  if (HEX.test(mine) && location.protocol !== 'file:') {
    (window.requestIdleCallback || function (f) { setTimeout(f, 2000); })(check, { timeout: 5000 });
    document.addEventListener('visibilitychange', function () { if (!document.hidden) check(); });
    setInterval(check, 600000);
  }
})();

/* dctx explainer animations: five canvas scenes for <figure data-anim> blocks made by
   tools/website/pages/animations.py. The prose fallback is HTML in the figure; this file only draws.
   Time is a frame counter (60 frames = one nominal second), never the clock, and nothing is random,
   so every frame is reproducible. Primitives mirror hero.js (sm, clamp, the colour tokens, JetBrains
   Mono labels). A scene starts when it scrolls into view; with motion off it draws its last frame. */
(function () {
  'use strict';
  var figs = [].slice.call(document.querySelectorAll('figure[data-anim]'));
  if (!figs.length || !window.requestAnimationFrame) return;
  var root = document.documentElement, reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  var C = {}, W0 = 520, H0 = 300, FPS = 60, players = [];
  function motion() { return root.classList.contains('motion') && !reduce.matches; }
  function clamp(v, a, b) { return v < a ? a : v > b ? b : v; }
  function sm(a, b, x) { var t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); }
  function lerp(a, b, t) { return a + (b - a) * t; }
  function typed(s, p) { return s.slice(0, Math.round(s.length * clamp(p, 0, 1))); }
  function num(n) { return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ','); }
  function colors() {
    var s = getComputedStyle(root);
    ['bg', 'surface', 'surface-2', 'ink', 'muted', 'line', 'brand', 'brand-text', 'signal', 'relation', 'ok', 'warn'].forEach(function (k) { C[k] = s.getPropertyValue('--' + k).trim(); });
  }

  /* ---- drawing kit, in the 520 x 300 logical space ---- */
  function kit(ctx, vectors) {
    var g = { ctx: ctx };
    g.font = function (s, w) { ctx.font = (w || 400) + ' ' + s + 'px "JetBrains Mono", ui-monospace, monospace'; };
    g.tx = function (s, x, y, col, a, size, w, al) {
      if (a <= .01 || !s) return;
      ctx.globalAlpha = Math.min(1, a); ctx.fillStyle = C[col] || col; g.font(size || 9, w); ctx.textAlign = al || 'left'; ctx.fillText(s, x, y);
    };
    g.rr = function (x, y, w, h, r) {
      r = Math.min(r, w / 2, h / 2); ctx.beginPath(); ctx.moveTo(x + r, y);
      ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
    };
    // o: f fill token, fa fill alpha, s stroke token, sa stroke alpha, r radius, lw line width, d dash
    g.box = function (x, y, w, h, o) {
      if (w <= 0 || h <= 0) return;
      g.rr(x, y, w, h, o.r == null ? 4 : o.r);
      if (o.f && o.fa !== 0) { ctx.globalAlpha = o.fa == null ? 1 : o.fa; ctx.fillStyle = C[o.f]; ctx.fill(); }
      if (o.s && o.sa !== 0) { ctx.globalAlpha = o.sa == null ? 1 : o.sa; ctx.strokeStyle = C[o.s]; ctx.lineWidth = o.lw || 1; ctx.setLineDash(o.d || []); ctx.stroke(); ctx.setLineDash([]); }
    };
    g.ln = function (x1, y1, x2, y2, col, a, lw, d) {
      if (a <= .01) return;
      ctx.globalAlpha = a; ctx.strokeStyle = C[col]; ctx.lineWidth = lw || 1; ctx.setLineDash(d || []);
      ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke(); ctx.setLineDash([]);
    };
    g.dot = function (x, y, r, col, a) { if (a <= .01) return; ctx.globalAlpha = a; ctx.fillStyle = C[col]; ctx.beginPath(); ctx.arc(x, y, r, 0, 6.2832); ctx.fill(); };
    g.ring = function (x, y, r, col, a, lw) { if (a <= .01) return; ctx.globalAlpha = a; ctx.strokeStyle = C[col]; ctx.lineWidth = lw || 1.5; ctx.beginPath(); ctx.arc(x, y, r, 0, 6.2832); ctx.stroke(); };
    g.symbol = function (path, col, a, width) {
      if (a <= .01) return;
      var node = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      node.setAttribute('d', path); node.setAttribute('class', 'vector-' + col);
      node.setAttribute('opacity', String(Math.min(1, a))); node.setAttribute('stroke-width', String(width || 1));
      node.setAttribute('stroke-linecap', 'round'); node.setAttribute('stroke-linejoin', 'round'); vectors.appendChild(node);
    };
    g.arrow = function (x, y, col, a) {
      g.symbol('M' + x + ' ' + (y + 4) + 'v-8m-3 3 3-3 3 3', col, a, 1);
    };
    g.check = function (x, y, s, col, a, p) {
      if (a <= .01 || p <= 0) return;
      var pts = [[x - s, y], [x - s * .35, y + s * .65], [x + s, y - s * .75]], l1 = .38, q = Math.min(1, p / l1);
      var path = 'M' + pts[0][0] + ' ' + pts[0][1] + 'L' + lerp(pts[0][0], pts[1][0], q) + ' ' + lerp(pts[0][1], pts[1][1], q);
      if (p > l1) { q = (p - l1) / (1 - l1); path += 'L' + lerp(pts[1][0], pts[2][0], q) + ' ' + lerp(pts[1][1], pts[2][1], q); }
      g.symbol(path, col, a, Math.max(1.5, s / 3.5));
    };
    g.tag = function (s, x, y, col, a) { // a small outlined label, e.g. PLANNED
      if (a <= .01) return; g.font(7.5, 600); var w = ctx.measureText(s).width + 10;
      g.box(x, y - 7, w, 14, { f: 'surface', fa: a, s: col, sa: a, r: 3 }); g.tx(s, x + 5, y + .5, col, a, 7.5, 600);
      return w;
    };
    return g;
  }

  /* ---- 1. two scopes, one binary ---- */
  function card(g, x, y, title, col, t, s0, lines) {
    var p = sm(s0, s0 + .7, t), w = 220, h = 130, i, dy = (1 - p) * 6;
    g.box(x, y + dy, w, h, { f: 'surface-2', fa: .7 * p, s: col, sa: p, r: 6 });
    g.tx(title, x + 12, y + 16 + dy, col, p, 10.5, 600);
    for (i = 0; i < lines.length; i++) {
      var q = sm(s0 + .3 + i * .18, s0 + .7 + i * .18, t);
      g.tx(lines[i][0], x + 12 + (1 - q) * 6, y + 38 + i * 18 + dy, lines[i][1], q, 9);
    }
  }
  var twoScopes = {
    t: 11.5,
    steps: [[0, 'one machine'], [.8, 'the system dctx on PATH belongs to you'], [2.2, 'a project vendors its own dctx, pinned by its lockfile'],
            [4.2, 'same engine, different version'], [5.8, 'agents and MCP call the system binary; app code and CI call the project one'],
            [7.6, 'dctx doctor lists both']],
    draw: function (g, t, D) {
      var V = D.v || '1.3.0', P = D.pv || '1.2.0', a0 = sm(0, .6, t), p3 = sm(4.2, 5.2, t), p4 = sm(5.8, 6.8, t), p5 = sm(7.6, 8.2, t);
      g.box(8, 8, 504, 284, { s: 'line', sa: a0, r: 8 });
      g.tx('your machine', 20, 22, 'muted', a0, 9);
      card(g, 24, 34, 'system scope', 'brand-text', t, .8, [['/usr/local/bin/dctx', 'ink'], ['on PATH', 'muted'], ['version ' + V, 'ink'],
        ['owner: you and your agents', 'muted'], ['upgraded when you choose', 'muted']]);
      card(g, 276, 34, 'project scope', 'relation', t, 2.2, [['app/node_modules/.bin/dctx', 'ink'], ['or app/.venv/bin/dctx', 'muted'],
        ['version ' + P + ', pinned', 'ink'], ['owner: the project', 'muted'], ['upgraded by its lockfile', 'muted']]);
      if (p3 > 0) {
        g.ln(134, 166, 134, 178, 'signal', p3, 1.2); g.ln(386, 166, 386, 178, 'signal', p3, 1.2);
        g.ln(134, 178, lerp(134, 386, p3), 178, 'signal', p3, 1.2);
        g.box(165, 170, 190, 16, { f: 'surface', fa: p3, s: 'signal', sa: p3, r: 8 });
        g.tx('same engine · different version', 260, 178.5, 'signal', p3, 8.5, 500, 'center');
      }
      [64, 312].forEach(function (x) { g.arrow(x, 200, 'ink', p4); });
      g.tx('called by agents and MCP', 139, 202, 'ink', p4, 9, 400, 'center');
      g.tx('called by app code and CI', 391, 202, 'ink', p4, 9, 400, 'center');
      g.tx('never falls back to PATH', 386, 215, 'muted', p4, 8.5, 400, 'center');
      if (p5 > 0) {
        g.box(24, 226, 472, 58, { f: 'surface-2', fa: p5, s: 'line', sa: p5, r: 5 });
        g.tx(typed('$ dctx doctor', sm(7.8, 8.6, t)), 36, 240, 'ink', p5, 9, 500);
        g.tag('PLANNED', 430, 240, 'warn', p5);
        g.tx(typed('system   /usr/local/bin/dctx             ' + V, sm(8.8, 9.6, t)), 36, 256, 'muted', p5, 9);
        g.tx(typed('project  app/node_modules/.bin/dctx      ' + P, sm(9.4, 10.2, t)), 36, 271, 'muted', p5, 9);
      }
    }
  };

  /* ---- 2. cost = calls x context (illustration) ---- */
  var callsTimes = {
    t: 13,
    steps: [[0, 'every call re-reads the whole conversation'], [1.2, 'agent A: grep and file reads, many small results'], [3.3, 'agent B: dctx, two bounded pages, then done'],
            [8.4, 'agent A keeps calling; the conversation grows each time'], [9.4, 'shaded area = context re-read in total'], [11, 'calls cost more than output size']],
    draw: function (g, t) {
      var x0 = 132, slot = 46, uh = 2.6, B = 10, L, li, i, k;
      var lanes = [{ y: 126, name: 'grep + reads', add: 3, n: 8, col: 'relation' }, { y: 252, name: 'dctx', add: 9, n: 2, col: 'brand' }];
      g.tx('illustration · not a measured result', 508, 14, 'warn', sm(0, .5, t), 8.5, 500, 'right');
      var shade = sm(9.4, 10.4, t), totals = [];
      for (li = 0; li < 2; li++) {
        L = lanes[li];
        var a = sm(.2 + li * .2, .8 + li * .2, t), tot = 0, tops = [];
        g.tx(L.name, 16, L.y - 72, 'ink', a, 11, 600);
        g.tx(L.n + ' calls', 16, L.y - 56, 'muted', a, 9);
        g.ln(x0 - 6, L.y + .5, 508, L.y + .5, 'line', a, 1);
        for (i = 0; i < L.n; i++) {
          var s = 1.2 + i * 1, p = sm(s, s + .6, t), h = B + L.add * i;
          if (p <= 0) break;
          var bx = x0 + i * slot;
          g.box(bx, L.y - B * uh * p, 30, B * uh * p, { f: 'muted', fa: .3, r: 1 });
          for (k = 0; k < i; k++) {
            var y1 = L.y - (B + L.add * (k + 1)) * uh * p;
            g.box(bx, y1 + .5, 30, L.add * uh * p - 1, { f: L.col, fa: k === i - 1 ? .8 : .4, r: 1 });
          }
          if (p < 1) g.ln(bx - 2, L.y - h * uh * p, bx + 32, L.y - h * uh * p, 'signal', 1 - p, 2);
          g.tx('call ' + (i + 1), bx + 15, L.y + 10, 'muted', p, 8, 400, 'center');
          tot += h * p; tops.push([bx, L.y - h * uh * p]);
        }
        if (shade > 0 && tops.length) { // the area under the bar tops: everything the calls re-read
          var ctx = g.ctx; ctx.beginPath(); ctx.moveTo(x0 - 4, L.y);
          for (k = 0; k < tops.length; k++) { ctx.lineTo(tops[k][0] - 4, tops[k][1] - 3); ctx.lineTo(tops[k][0] + 34, tops[k][1] - 3); }
          ctx.lineTo(tops[tops.length - 1][0] + 34, L.y); ctx.closePath();
          ctx.globalAlpha = .14 * shade; ctx.fillStyle = C[L.col]; ctx.fill();
          ctx.globalAlpha = .8 * shade; ctx.strokeStyle = C[L.col]; ctx.lineWidth = 1; ctx.setLineDash([3, 3]); ctx.stroke(); ctx.setLineDash([]);
        }
        g.tx('re-read: ' + Math.round(tot) + ' units', 16, L.y - 40, shade > 0 ? L.col === 'brand' ? 'brand-text' : 'ink' : 'muted', a, 9, shade > 0 ? 600 : 400);
        totals.push(Math.round(tot));
      }
      var lg = sm(1.2, 1.8, t);
      g.box(16, 136, 10, 8, { f: 'muted', fa: .3 * lg, r: 1 }); g.tx('task + prompt', 30, 140.5, 'muted', lg, 8);
      g.box(16, 150, 10, 8, { f: 'relation', fa: .6 * lg, r: 1 }); g.tx('earlier results', 30, 154.5, 'muted', lg, 8);
      g.check(x0 + 2 * slot + 12, 222, 7, 'ok', sm(3.3, 3.8, t), sm(3.3, 3.9, t));
      g.tx('done after 2 calls', x0 + 2 * slot + 26, 222, 'ok', sm(3.4, 4, t), 9, 500);
      g.tx('same task: ' + totals[0] + ' vs ' + totals[1] + ' units re-read', 320, 286, 'ink', sm(11, 11.8, t), 9.5, 600, 'center');
    }
  };

  /* ---- 3. pack prefix stability ---- */
  var FILES = [['head', 30], ['README.md', 52], ['src/lib.rs', 58], ['src/main.rs', 60], ['src/store.rs', 64], ['src/util.rs', 58], ['tests/e2e.rs', 64], ['web/app.ts', 58]], EDIT = 6;
  function strip(g, y, t, s0, kind) {
    var x = 38, i, before = 0;
    for (i = 0; i < FILES.length; i++) {
      var p = sm(s0 + i * .16, s0 + .35 + i * .16, t), w = FILES[i][1], lab = FILES[i][0], col = 'brand', fa = .22;
      if (kind === 2) { if (i < EDIT) { col = 'ok'; fa = .2; } else fa = .5; }
      if (kind === 3) { fa = .5; if (i === 0) { col = 'warn'; lab = '12:04'; } }
      if (p > 0) {
        g.box(x + 1, y, (w - 2) * p, 24, { f: col, fa: fa * p, s: col, sa: p, r: 2 });
        g.tx(lab, x + w / 2, y + 12.5, 'ink', sm(.5, 1, p), 7, 400, 'center');
      }
      if (i < EDIT) before += w;
      x += w;
    }
    return before;
  }
  function meter(g, y, frac, p, col, label) {
    g.box(38, y, 444, 6, { f: 'line', fa: p, r: 3 });
    g.box(38, y, 444 * frac * p, 6, { f: col, fa: p, r: 3 });
    g.tx(label, 38, y + 17, col === 'ok' ? 'ok' : col, p, 9, 600);
  }
  var prefixStable = {
    t: 11.5,
    steps: [[0, 'turn 1: the pack is sent, the provider caches it'], [3, 'you edit one file'], [4.2, 'turn 2: same order, so everything before the edit is unchanged'],
            [6.2, 'the cache reuses the unchanged prefix'], [8, 'if the pack began with a timestamp, nothing would be reused'], [10.2, 'edit a file, keep the cache']],
    draw: function (g, t) {
      var total = 444, a1 = sm(0, .4, t), pc = sm(1.9, 2.6, t), pe = sm(3, 3.6, t), i, x;
      g.tx('turn 1 · pack sent', 38, 44, 'muted', a1, 9);
      g.tx('illustrative sizes', 482, 44, 'warn', a1, 8.5, 500, 'right');
      var before = strip(g, 52, t, .2, 1);
      if (pc > 0) { g.ln(38, 82, 38 + total * pc, 82, 'ok', pc, 2); g.tx('cached by the provider', 38, 92, 'ok', pc, 8.5); }
      if (pe > 0) {
        for (x = 38, i = 0; i < EDIT; i++) x += FILES[i][1];
        g.box(x + 1, 50, FILES[EDIT][1] - 2, 28, { s: 'warn', sa: pe, lw: 2, r: 3 });
        g.tx('edited', x + FILES[EDIT][1] / 2, 92, 'warn', pe, 8.5, 600, 'center');
      }
      g.tx('turn 2 · after the edit', 38, 116, 'muted', sm(4.2, 4.6, t), 9);
      strip(g, 124, t, 4.2, 2);
      var pm = sm(6.2, 7.4, t);
      if (pm > 0) {
        meter(g, 156, before / total, pm, 'ok', 'prefix reused: ' + Math.round(100 * before / total * pm) + '% of the bytes');
        g.tx('re-sent', 38 + before + (total - before) / 2, 116, 'brand-text', pm, 8.5, 600, 'center');
      }
      var p3 = sm(8, 8.4, t);
      g.tx('same edit, but the pack starts with a timestamp', 38, 196, 'muted', p3, 9);
      strip(g, 204, t, 8, 3);
      var pz = sm(9.4, 10, t);
      if (pz > 0) meter(g, 236, 0, pz, 'warn', 'prefix reused: 0%, the first byte changed');
      g.tx('dctx output has no timestamps by default · prefix tiers are planned', 260, 284, 'muted', sm(10.2, 10.8, t), 8.5, 400, 'center');
    }
  };

  /* ---- 4. the giant folder (planned catalog and shards) ---- */
  var TREE = [['photos', 50], ['datasets', 44], ['archive', 30], ['repos', 30, [['billing', 5], ['shared-ui', 5], ['infra', 6], ['ml-lab', 6], ['web', 4], ['tools', 4]]],
              ['videos', 22], ['backups', 14], ['notes', 6], ['misc', 4]], LIT = ['billing', 'shared-ui'], RECTS = [];
  function sum(a) { var s = 0; for (var i = 0; i < a.length; i++) s += a[i][1]; return s; }
  function lay(items, x, y, w, h, out) { // deterministic binary split along the longer side
    if (items.length === 1) {
      var it = items[0];
      if (it[2]) { out.push({ n: it[0], x: x, y: y, w: w, h: h, parent: 1 }); lay(it[2], x + 3, y + 13, w - 6, h - 16, out); }
      else out.push({ n: it[0], x: x, y: y, w: w, h: h });
      return;
    }
    var tot = sum(items), acc = 0, k = 0;
    while (k < items.length - 1 && acc + items[k][1] <= tot / 2) acc += items[k++][1];
    if (k === 0) acc = items[k++][1];
    var f = acc / tot, A = items.slice(0, k), Bv = items.slice(k);
    if (w >= h) { lay(A, x, y, w * f, h, out); lay(Bv, x + w * f, y, w * (1 - f), h, out); }
    else { lay(A, x, y, w, h * f, out); lay(Bv, x, y + h * f, w, h * (1 - f), out); }
  }
  lay(TREE, 16, 40, 300, 232, RECTS);
  var SHARDS = RECTS.filter(function (r) { return !r.parent; }).length;
  var IDX = [[.25, .3], [.7, .25], [.5, .55], [.2, .75], [.78, .7], [.45, .85]], BARS = [120, 92, 136, 76, 108, 60];
  var giant = {
    t: 13,
    steps: [[0, 'a 200 GB folder'], [1.4, 'catalog (planned): names and sizes only, no contents read'], [4.2, 'a task arrives'],
            [5.2, 'two shards match the task'], [6.6, 'only those two get symbol indexes'], [8.6, 'a bounded page comes out of them'],
            [11, 'index what the task touches, never copy the tree']],
    draw: function (g, t) {
      var a0 = sm(0, .8, t), ps = sm(1.4, 3.8, t), sx = 16 + 300 * ps, pt = sm(4.2, 5, t), pl = sm(5.2, 6.4, t), pi = sm(6.6, 8.4, t), pp = sm(8.6, 9.4, t), i, r, lit = [];
      g.tx('~/data · 200 GB', 16, 26, 'ink', a0, 11, 600);
      for (i = 0; i < RECTS.length; i++) {
        r = RECTS[i];
        var a = sm(.1 + i * .05, .6 + i * .05, t), cat = ps > 0 && r.x + r.w / 2 < sx, on = LIT.indexOf(r.n) >= 0;
        g.box(r.x + 1, r.y + 1, r.w - 2, r.h - 2, { f: r.parent ? null : 'surface-2', fa: .8 * a, s: cat ? 'brand-text' : 'line', sa: a * (cat ? .7 : 1), r: 2 });
        if (on && pl > 0) { g.box(r.x + 1, r.y + 1, r.w - 2, r.h - 2, { f: 'signal', fa: .28 * pl, s: 'signal', sa: pl, lw: 1.6, r: 2 }); lit.push(r); }
        if (r.w > 34) g.tx(r.n + (r.parent ? '/' : ''), r.x + 5, r.y + 9, on && pl > 0 ? 'ink' : 'muted', a, 7.5, on ? 600 : 400);
      }
      if (ps > 0 && ps < 1) g.ln(sx, 38, sx, 274, 'brand', 1, 1.5);
      var ca = sm(1.4, 2, t);
      g.tx('catalog', 336, 52, 'brand-text', ca, 10, 600); g.tag('PLANNED', 386, 52, 'warn', ca);
      g.tx('names and sizes only: ' + num(Math.round(1240000 * ps)), 336, 67, 'muted', ca, 8);
      if (pt > 0) {
        g.box(334, 82 + (1 - pt) * 8, 170, 42, { f: 'surface', fa: pt, s: 'signal', sa: pt, r: 5 });
        g.tx('task', 344, 95 + (1 - pt) * 8, 'muted', pt, 8);
        g.tx('"fix invoice rounding"', 344, 111 + (1 - pt) * 8, 'ink', pt, 9.5, 500);
      }
      for (i = 0; i < lit.length; i++) {
        r = lit[i];
        g.ln(334, 103, lerp(334, r.x + r.w / 2, pl), lerp(103, r.y + r.h / 2, pl), 'signal', pl * .8, 1, [3, 3]);
        if (pi > 0) {
          for (var k = 0; k < IDX.length; k++) {
            var q = sm(k / IDX.length, (k + 1) / IDX.length, pi), px = r.x + 4 + IDX[k][0] * (r.w - 8), py = r.y + 13 + IDX[k][1] * (r.h - 17);
            if (k) g.ln(r.x + 4 + IDX[k - 1][0] * (r.w - 8), r.y + 13 + IDX[k - 1][1] * (r.h - 17), px, py, 'relation', q * .8, 1);
            g.dot(px, py, 2.2, 'brand', q);
          }
        }
      }
      g.tx('repos/billing · repos/shared-ui', 336, 136, 'signal', pl, 8);
      g.tx('symbol index: ' + LIT.length + ' shards', 336, 150, 'signal', pi, 9, 600);
      if (pp > 0) {
        g.box(336, 174, 168, 92, { s: 'brand', sa: pp, lw: 1.2, d: [5, 4], r: 2 });
        g.tx('≤ BUDGET · STABLE ORDER', 336, 166, 'brand-text', pp, 7.5, 600);
        for (i = 0; i < BARS.length; i++) {
          var e = sm(9 + i * .14, 10 + i * .14, t), s = lit[i % 2] || RECTS[0];
          if (e <= 0) continue;
          var tx0 = 348, ty0 = 184 + i * 13, bx = lerp(s.x + s.w / 2, tx0, e), by = lerp(s.y + s.h / 2, ty0, e);
          g.box(bx, by, lerp(6, BARS[i] + 12, e), 6, { f: i % 2 ? 'brand' : 'signal', fa: .9, r: 1 });
        }
      }
      g.tx(LIT.length + ' of ' + SHARDS + ' shards indexed', 336, 280, 'ink', sm(10.6, 11.2, t), 9, 600);
      g.tx('nothing copied; the index covers what the task touches', 16, 287, 'muted', sm(11, 11.6, t), 8.5);
    }
  };

  /* ---- 5. the safe path ---- */
  var safe = {
    t: 14,
    labels: [['new', 'user'], ['install', 'system binary'], ['one', 'project'], ['lean MCP', 'profile'], ['first', 'call'], ['measure', '']],
    steps: [[0, 'a new user'], [2.5, 'install the system binary'], [4.4, 'pick one project'], [6.3, 'connect the lean MCP profile'],
            [8.2, 'make the first call and read its last line'], [10.1, 'measure'], [12, 'measure before you change anything']],
    lines: function (D) {
      return [
        ['# you have: one agent, one repository, no plan yet', '# the safe path: one project, the system binary,', '# the lean MCP profile; measure before you change anything'],
        ["# download your platform's archive from GitHub Releases", '$ sha256sum -c SHA256SUMS --ignore-missing', '$ dctx --version', 'dctx ' + (D.v || '1.3.0')],
        ['$ cd ~/code/app', '# one project root, not your home folder or a 200 GB tree', '# the system binary serves it; nothing installed per project'],
        ['$ dctx mcp --profile lean /abs/path/to/app', '# two tools, find and read, with compact descriptions:', '# fewer tool definitions in every turn'],
        ["$ dctx pack . --for '" + (D.q || 'greet store') + "' --budget " + (D.b || '800'), '# ... the bounded page ...', '!' + (D.end || '# end body_tokens=223 budget=800 dropped=0'), '# that last line is your first measurement'],
        ['# per task, count: calls, start-up tokens, total input', '# run the same task with grep + reads and compare', '# receipts that count this for you are planned']
      ];
    },
    draw: function (g, t, D) {
      var S = [.6, 2.5, 4.4, 6.3, 8.2, 10.1], L = this.lines(D), cur = -1, i, j, x, f = Math.round(t * FPS);
      for (i = 0; i < S.length; i++) if (t >= S[i]) cur = i;
      var pk = sm(12, 12.8, t);
      for (i = 0; i < S.length; i++) {
        x = 44 + i * 86.4;
        if (i) { var pl = sm(S[i] - .5, S[i], t); g.ln(x - 75, 54, x - 11, 54, 'line', 1, 2); g.ln(x - 75, 54, lerp(x - 75, x - 11, pl), 54, 'ok', pl, 2); }
        var on = t >= S[i], here = i === cur && pk === 0;
        g.ring(x, 54, 9, on ? (here ? 'brand' : 'ok') : 'line', 1, 1.6);
        if (on) g.dot(x, 54, 5.5, here ? 'brand' : 'ok', sm(S[i], S[i] + .3, t));
        if (here) g.ring(x, 54, 13 + 2 * Math.sin(f / 9), 'brand', .35, 1);
        g.tx(this.labels[i][0], x, 78, on ? 'ink' : 'muted', 1, 8.5, here ? 600 : 400, 'center');
        g.tx(this.labels[i][1], x, 90, on ? 'ink' : 'muted', 1, 8.5, here ? 600 : 400, 'center');
      }
      g.box(24, 106, 472, 144, { f: 'surface-2', fa: .7, s: 'line', r: 6 });
      if (cur >= 0) {
        var a = sm(S[cur], S[cur] + .3, t), s0 = S[cur];
        g.tx('step ' + (cur + 1) + ' of 6 · ' + this.steps[cur][1], 38, 122, 'brand-text', a, 9.5, 600);
        for (j = 0; j < L[cur].length; j++) {
          var s = L[cur][j], hl = s.charAt(0) === '!', p = sm(s0 + .2 + j * .28, s0 + .6 + j * .28, t), y = 146 + j * 22;
          if (hl) { s = s.slice(1); g.box(32, y - 10, 456, 20, { f: 'signal', fa: .14 * p, s: 'signal', sa: p, r: 3 }); }
          g.tx(typed(s, p * 1.4), 40, y, hl ? 'signal' : s.charAt(0) === '#' ? 'muted' : 'ink', a, 9, hl || s.charAt(0) === '$' ? 500 : 400);
        }
      }
      g.dot(476, 54, 12 * pk, 'ok', pk); g.check(476, 54, 6, 'surface', pk, pk);
      g.tx('Measure before you change anything.', 260, 276, 'ink', pk, 11, 600, 'center');
    }
  };

  var SCENES = { two_scopes: twoScopes, calls_times_context: callsTimes, prefix_stability: prefixStable, giant_folder: giant, safe_path: safe };

  /* ---- player: one per figure ---- */
  function Player(fig) {
    var S = SCENES[fig.getAttribute('data-anim')], cv = fig.querySelector('canvas'), ctx = cv && cv.getContext && cv.getContext('2d');
    if (!S || !ctx) return null;
    var vectors = fig.querySelector('.anim-vectors'), g = kit(ctx, vectors), stage = cv.parentNode, stepEl = fig.querySelector('[data-step]'), ctl = fig.querySelector('[data-ctl]');
    var pb = fig.querySelector('[data-pause]'), pl = pb && pb.querySelector('[data-l]'), rb = fig.querySelector('[data-replay]');
    var N = Math.round(S.t * FPS), f = 0, paused = false, seen = !('IntersectionObserver' in window), raf = 0, sc = 1, DPR = 1, last = -1;
    function size() {
      var w = Math.max(1, stage.getBoundingClientRect().width);
      DPR = Math.min(2, window.devicePixelRatio || 1); sc = w / W0;
      cv.width = Math.round(w * DPR); cv.height = Math.round(w * H0 / W0 * DPR);
    }
    function render() {
      var t = f / FPS, i = 0;
      vectors.replaceChildren();
      ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.globalAlpha = 1; ctx.clearRect(0, 0, cv.width, cv.height);
      ctx.setTransform(DPR * sc, 0, 0, DPR * sc, 0, 0); ctx.textBaseline = 'middle'; ctx.lineCap = 'round';
      S.draw(g, t, fig.dataset); ctx.globalAlpha = 1;
      while (i + 1 < S.steps.length && S.steps[i + 1][0] <= t) i++;
      if (i !== last && stepEl) { last = i; stepEl.textContent = (i + 1) + '/' + S.steps.length + ' · ' + S.steps[i][1]; }
    }
    function ui() {
      if (ctl) ctl.hidden = !motion();
      if (pb) { if (pl) pl.textContent = paused ? 'Play' : 'Pause'; pb.setAttribute('aria-pressed', String(paused)); pb.disabled = f >= N; }
    }
    function tick() {
      raf = 0;
      if (paused || !seen || document.hidden || !motion()) return;
      if (f < N) { f++; render(); }
      if (f < N) raf = requestAnimationFrame(tick); else ui();
    }
    function go() { if (!raf && !paused && seen && f < N && motion()) raf = requestAnimationFrame(tick); ui(); }
    function reset() { cancelAnimationFrame(raf); raf = 0; paused = false; f = motion() ? 0 : N; render(); go(); }
    if (pb) pb.addEventListener('click', function () { paused = !paused; go(); });
    if (rb) rb.addEventListener('click', function () { cancelAnimationFrame(raf); raf = 0; f = 0; paused = false; render(); go(); });
    if ('IntersectionObserver' in window) new IntersectionObserver(function (en) { seen = en[0].isIntersecting; go(); }, { threshold: .35 }).observe(stage);
    if ('ResizeObserver' in window) new ResizeObserver(function () { size(); render(); }).observe(stage);
    size(); reset();
    return { reset: reset, redraw: render, go: go };
  }

  function each(m) { players.forEach(function (p) { p[m](); }); }
  function boot() {
    colors();
    figs.forEach(function (fig) { var p = Player(fig); if (p) players.push(p); });
  }
  document.addEventListener('dctx:motion', function () { each('reset'); });
  document.addEventListener('dctx:theme', function () { colors(); each('redraw'); });
  document.addEventListener('visibilitychange', function () { each('go'); });
  if (window.matchMedia) window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function () { colors(); each('redraw'); });
  if (document.readyState === 'complete') boot(); else window.addEventListener('load', boot);
})();
