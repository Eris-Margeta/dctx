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
    mt.textContent = 'Motion: ' + (on ? 'on' : 'off');
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
    return pre.textContent.replace(/\n$/, '');
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

  var menu = document.querySelector('.menu');
  if (menu) document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && menu.open) { menu.open = false; menu.querySelector('summary').focus(); } });
})();

/* Terminal playback. The full transcript is already in the page (screen readers and no-JS
   visitors read it as is); with motion on, commands are typed and output lines stream in. */
(function () {
  'use strict';
  var root = document.documentElement, reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  function motion() { return root.classList.contains('motion') && !reduce.matches; }
  function Term(el) {
    this.el = el; this.lines = [].slice.call(el.querySelectorAll('.ln')); this.box = el.querySelector('.scroll'); this.timer = 0; this.i = 0;
    var self = this, btn = el.querySelector('[data-replay]');
    if (btn) { btn.hidden = false; btn.addEventListener('click', function () { self.play(); }); }
  }
  Term.prototype.reset = function () {
    clearTimeout(this.timer); this.i = 0;
    this.lines.forEach(function (l) { l.classList.remove('on', 'typing', 'caret', 'caret-wait'); });
  };
  Term.prototype.finish = function () {
    this.reset(); this.el.removeAttribute('data-live');
  };
  Term.prototype.show = function (l) {
    var b = this.box; if (!b) return;
    var over = l.getBoundingClientRect().bottom + 12 - b.getBoundingClientRect().bottom;
    if (over > 0) b.scrollTop += over;
  };
  Term.prototype.play = function () {
    if (!motion()) { this.finish(); return; }
    this.el.setAttribute('data-live', ''); this.reset(); if (this.box) this.box.scrollTop = 0;
    var self = this;
    function next() {
      if (document.hidden) { self.timer = setTimeout(next, 300); return; }
      if (self.i >= self.lines.length) { var last = self.lines[self.lines.length - 1]; if (last) last.classList.add('caret'); return; }
      var l = self.lines[self.i++];
      if (l.classList.contains('cmd')) {
        var n = l.textContent.length, d = n * 28;
        l.style.setProperty('--n', n); l.style.setProperty('--t', d + 'ms'); l.classList.add('typing'); self.show(l);
        self.timer = setTimeout(function () { l.classList.remove('typing'); l.classList.add('on'); self.timer = setTimeout(next, 300); }, d);
      } else {
        l.classList.add('on'); self.show(l);
        self.timer = setTimeout(next, l.classList.contains('meta') ? 260 : 24);
      }
    }
    next();
  };
  var terms = [].slice.call(document.querySelectorAll('.term[data-play]')).map(function (el) { return new Term(el); });
  terms.forEach(function (t) {
    var start = t.el.getAttribute('data-play');
    if (!motion()) return;
    if (start === 'scene') {
      t.el.setAttribute('data-live', '');
      if (t.lines[0]) t.lines[0].classList.add('caret-wait');
      var fallback = setTimeout(function () { t.play(); }, 14000);
      document.addEventListener('dctx:scene', function () { clearTimeout(fallback); t.play(); });
    } else if ('IntersectionObserver' in window) {
      t.el.setAttribute('data-live', '');
      var io = new IntersectionObserver(function (en) { if (en[0].isIntersecting) { io.disconnect(); t.play(); } }, { threshold: .3 });
      io.observe(t.el);
    }
  });
  document.addEventListener('dctx:motion', function () { terms.forEach(function (t) { if (!motion()) t.finish(); }); });
})();

/* dctx hero scene: repository -> policy -> graph -> bounded page, drawn from the captured demo data
   (the JSON in #scene-data, the same geometry as the OG images and the poster). The flow field is
   the one from the earlier site. Plays once; with motion off it draws the final frame. */
(function () {
  'use strict';
  var stage = document.querySelector('.stage[data-scene]'), data = document.getElementById('scene-data');
  if (!stage || !data || !window.requestAnimationFrame) return;
  var cv = stage.querySelector('canvas'), ctx = cv && cv.getContext && cv.getContext('2d');
  if (!ctx) return;
  var G = JSON.parse(data.textContent), root = document.documentElement;
  var steps = [].slice.call(document.querySelectorAll('.steps li')), cap = stage.querySelector('[data-cap]');
  var replayBtn = stage.querySelector('[data-replay]'), perf = /[?&]perf\b/.test(location.search), samples = [];
  var C = {}, W = 1, H = 1, DPR = 1, sx = 1, sy = 1, sc = 1, t0 = 0, pausedAt = 0, raf = 0, idleTimer = 0, done = false, visible = true;
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  function motion() { return root.classList.contains('motion') && !reduce.matches; }
  function now() { return performance.now(); }
  function clamp(v, a, b) { return v < a ? a : v > b ? b : v; }
  function sm(a, b, x) { var t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); }
  function colors() {
    var s = getComputedStyle(root);
    ['bg', 'surface', 'ink', 'muted', 'line', 'brand', 'brand-text', 'signal', 'relation', 'warn'].forEach(function (k) { C[k] = s.getPropertyValue('--' + k).trim(); });
  }

  /* ---- flow field (deterministic gradient noise, LCG 1664525/1013904223, seed 1337) ---- */
  var perm = new Uint8Array(512);
  (function () { var p = [], i, s = 1337; for (i = 0; i < 256; i++) p[i] = i; for (i = 255; i > 0; i--) { s = (s * 1664525 + 1013904223) >>> 0; var j = (s / 4294967296 * (i + 1)) | 0, t = p[i]; p[i] = p[j]; p[j] = t; } for (i = 0; i < 512; i++) perm[i] = p[i & 255]; })();
  function fade(t) { return t * t * t * (t * (t * 6 - 15) + 10); }
  function grad(h, x, y) { switch (h & 7) { case 0: return x + y; case 1: return -x + y; case 2: return x - y; case 3: return -x - y; case 4: return x; case 5: return -x; case 6: return y; default: return -y; } }
  function lerp(a, b, t) { return a + (b - a) * t; }
  function noise(x, y) {
    var X = Math.floor(x), Y = Math.floor(y); x -= X; y -= Y; X &= 255; Y &= 255;
    var u = fade(x), v = fade(y), A = perm[X] + Y, B = perm[X + 1] + Y;
    return lerp(lerp(grad(perm[A], x, y), grad(perm[B], x - 1, y), u), lerp(grad(perm[A + 1], x, y - 1), grad(perm[B + 1], x - 1, y - 1), u), v);
  }
  function angle(x, y, T) { return noise(x * .0042 + T * .03, y * .0042 - T * .02) * 3.4 + noise(x * .0013 - T * .012, y * .0013 + T * .01) * 4.6 + .6; }
  var P = { x: 0, y: 0, tx: 0, ty: 0, pres: 0, target: 0, last: 0 }, pts = [];
  function seedField() {
    pts = []; var s = 7, sp = clamp(W / 34, 18, 30);
    function r() { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; }
    for (var y = 0; y < H + sp; y += sp) for (var x = 0; x < W + sp; x += sp) pts.push(x + (r() - .5) * sp * .6, y + (r() - .5) * sp * .6, r());
  }
  function field(T) {
    P.x += (P.tx - P.x) * .14; P.y += (P.ty - P.y) * .14; P.pres += (P.target - P.pres) * (P.target > P.pres ? .08 : .03);
    var R = clamp(W * .16, 70, 160), R2 = R * R;
    ctx.lineCap = 'round'; ctx.lineWidth = 1; ctx.strokeStyle = C.muted;
    for (var pass = 0; pass < 2; pass++) {
      ctx.globalAlpha = pass ? .55 : .16; ctx.strokeStyle = pass ? C.brand : C.muted; ctx.beginPath();
      for (var i = 0; i < pts.length; i += 3) {
        var x = pts[i] / sx, y = pts[i + 1] / sy, a = angle(x, y, T), dx = pts[i] - P.x, dy = pts[i + 1] - P.y, d2 = dx * dx + dy * dy, e = 0;
        if (P.pres > .02 && d2 < R2 * 5) { e = Math.exp(-d2 / (2 * R2)) * P.pres; a = Math.atan2(Math.sin(a) * (1 - e) + dx * e / Math.sqrt(d2 + 1), Math.cos(a) * (1 - e) - dy * e / Math.sqrt(d2 + 1)); }
        if ((e > .25) !== !!pass) continue;
        var L = (7 + 6 * pts[i + 2] + 14 * e) * Math.max(.7, sc), cx = Math.cos(a) * L / 2, cy = Math.sin(a) * L / 2;
        ctx.moveTo(pts[i] - cx, pts[i + 1] - cy); ctx.lineTo(pts[i] + cx, pts[i + 1] + cy);
      }
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  /* ---- the scene (mirrors draw_scene in tools/website/og.py) ---- */
  var mods = {};
  G.nodes.forEach(function (n, i) { if (n.id.indexOf('m:') === 0) mods[n.id.slice(2)] = i; });
  function X(x) { return x * sx; } function Y(y) { return y * sy; }
  function dot(x, y, r, col, a) { ctx.globalAlpha = a; ctx.fillStyle = col; ctx.beginPath(); ctx.arc(X(x), Y(y), r, 0, 6.2832); ctx.fill(); }
  function text(s, x, y, col, a, f) { ctx.globalAlpha = a; ctx.fillStyle = col; ctx.font = f; ctx.fillText(s, x, y); }
  function scene(t) {
    var pp = sm(3, 5, t), pg = sm(5, 7.5, t), pq = sm(7, 8, t), pb = sm(9, 11.5, t), N = G.nodes, i, k;
    var fs = W < 560 ? 9 : 10.5, mono = '500 ' + fs + 'px "JetBrains Mono", ui-monospace, monospace', small = '400 ' + (fs - .5) + 'px "JetBrains Mono", ui-monospace, monospace';
    ctx.textBaseline = 'middle';
    if (pg > 0) {
      ctx.lineWidth = 1; ctx.strokeStyle = C.line; ctx.globalAlpha = pg; ctx.beginPath();
      G.tree.forEach(function (e) { ctx.moveTo(X(N[e[0]].x), Y(N[e[0]].y)); ctx.lineTo(X(N[e[1]].x), Y(N[e[1]].y)); });
      ctx.stroke();
      G.edges.forEach(function (e, k) {
        var r = sm(5 + k * .15, 6.2 + k * .15, t); if (r <= 0) return;
        var a = N[e[0]], b = N[e[1]], hot = pq > 0 && (e[0] === G.hit || e[1] === G.hit);
        ctx.globalAlpha = .8; ctx.lineWidth = hot ? 2 : 1; ctx.strokeStyle = hot ? C.signal : e[2] === 'calls' ? C.relation : C.muted;
        ctx.beginPath(); ctx.moveTo(X(a.x), Y(a.y)); ctx.lineTo(X(a.x + (b.x - a.x) * r), Y(a.y + (b.y - a.y) * r)); ctx.stroke();
      });
      for (i = 0; i < N.length; i++) {
        var n = N[i], m = n.id.indexOf('m:') === 0, code = /^s:(src|web|tools)\//.test(n.id), col = code ? C.brand : C.muted, r = (m ? 4.5 : 3) * Math.max(.8, sc * 1.4) * pg;
        var nb = G.nbrs.indexOf(i) >= 0;
        if (pq > 0 && i === G.hit) { col = C.signal; r *= 1 + .7 * pq; }
        else if (pq > 0 && nb) col = C.signal;
        dot(n.x, n.y, r, col, .95);
        if (pq > 0 && nb && !m) text(n.id.split('#').pop(), X(n.x) + 7, Y(n.y) - 8, C.signal, pq, small);
      }
      if (pq > 0) { var h = N[G.hit]; text('find greet → ' + h.id, X(h.x) - 4, Y(h.y) + 16, C.signal, pq, small); }
    }
    for (k = 0; k < G.files.length; k++) {
      var f = G.files[k], ap = sm(.2 + k * .18, .8 + k * .18, t); if (ap <= 0) continue;
      var x = f.x, y = f.y, al = ap, c = C.ink, lab = f.p, mi = mods[f.p];
      if (f.s === 'ignored' || f.s === 'binary') {
        var an = angle(x, y, 0); x += Math.cos(an) * 120 * pp; y += Math.sin(an) * 120 * pp; al *= 1 - pp;
        if (pp > .05) { c = C.warn; lab += f.s === 'ignored' ? '  · ignored' : '  · binary'; }
      } else if (f.s === 'listed-not-read') {
        if (pp > .05) { c = C.warn; lab += '  · name only, never read'; }
        x += (24 - x) * pg; y += (G.h - 52 - y) * pg;
      }
      if (pg > 0 && mi !== undefined) { x += (N[mi].x - x) * pg; y += (N[mi].y - y) * pg; al *= 1 - .45 * pg; }
      if (al > .02) text(lab, X(x) + 8, Y(y) + (pg > .5 ? 9 : 0), c, al, pg < .5 ? mono : small);
    }
    if (pb > 0) {
      var F = G.frame, x0 = X(F.x), y0 = Y(F.y), w = X(F.w), hh = Y(F.h);
      ctx.globalAlpha = pb; ctx.strokeStyle = C.brand; ctx.lineWidth = 1.2; ctx.setLineDash([5, 4]); ctx.strokeRect(x0 + .5, y0 + .5, w, hh); ctx.setLineDash([]);
      text('≤ BUDGET · STABLE ORDER', x0, y0 - 10, C['brand-text'], pb, small);
      for (i = 0; i < G.bars.length; i++) {
        var B = G.bars[i], e2 = sm(9 + i * .07, 10.2 + i * .07, t); if (e2 <= 0) continue;
        var s = N[B.n], tx = F.x + 14, ty = F.y + 16 + i * 13, bx = s.x + (tx - s.x) * e2, by = s.y + (ty - s.y) * e2, bw = 6 + (B.w - 6) * e2;
        ctx.globalAlpha = .9; ctx.fillStyle = B.h ? C.muted : B.n === G.hit ? C.signal : C.brand;
        ctx.fillRect(X(bx), Y(by), Math.max(2, X(bw)), Math.max(2, Y(6)));
      }
      var c2 = G.counts;
      text('body_tokens=' + Math.round(c2.body_tokens * sm(9.4, 11.6, t)) + ' budget=' + c2.budget + ' dropped=' + c2.dropped, x0, y0 + hh + 14, C.muted, pb, small);
    }
    ctx.globalAlpha = 1;
  }
  function caption(t) {
    var c = G.counts, i = t < 3 ? 0 : t < 5 ? 1 : t < 9 ? 2 : 3;
    var s = [c.disk + ' files', c.disk + ' → ' + c.listed + ' listed · ' + c.read + ' read', c.nodes + ' nodes · ' + c.edges + ' call and import edges', c.body_tokens + ' of ' + c.budget + ' tokens'][i];
    if (cap) cap.textContent = (i + 1) + ' ' + ['repository', 'policy', 'graph', 'bounded page'][i] + ' · ' + s;
    steps.forEach(function (li, j) { li.classList.toggle('on', j === i); });
  }

  function size() {
    var r = stage.getBoundingClientRect();
    DPR = Math.min(window.innerWidth < 720 ? 1.5 : 2, window.devicePixelRatio || 1);
    W = Math.max(1, r.width); H = Math.max(1, r.height); sx = W / G.w; sy = H / G.h; sc = Math.min(sx, sy);
    cv.width = Math.round(W * DPR); cv.height = Math.round(H * DPR); ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    seedField(); if (!P.target) { P.x = P.tx = W * .5; P.y = P.ty = H * .5; }
  }
  function render(t, T) {
    var a = perf ? now() : 0;
    ctx.clearRect(0, 0, W, H); field(T); scene(t); caption(t);
    if (perf) { samples.push(now() - a); if (samples.length === 120) { samples.sort(function (p, q) { return p - q; }); console.log('dctx hero: median frame ' + samples[60].toFixed(2) + ' ms, p95 ' + samples[114].toFixed(2) + ' ms'); samples = []; } }
  }
  function loop() {
    raf = 0; if (!visible || document.hidden) return;
    var t = (now() - t0) / 1000;
    render(Math.min(t, 13), t);
    if (t >= 12 && !done) { done = true; document.dispatchEvent(new CustomEvent('dctx:scene')); }
    if (t < 13 || now() - P.last < 1500 || P.pres > .03) raf = requestAnimationFrame(loop);
    else idle();
  }
  function idle() { clearTimeout(idleTimer); if (!motion()) return; idleTimer = setTimeout(function () { if (!raf) raf = requestAnimationFrame(loop); }, 400); }
  function still() { cancelAnimationFrame(raf); raf = 0; clearTimeout(idleTimer); render(13, 0); stage.classList.add('live'); }
  function play() {
    cancelAnimationFrame(raf); raf = 0; clearTimeout(idleTimer);
    if (!motion()) { still(); document.dispatchEvent(new CustomEvent('dctx:scene')); return; }
    done = false; t0 = now(); stage.classList.add('live'); raf = requestAnimationFrame(loop);
  }
  function wake() { if (motion() && !raf) raf = requestAnimationFrame(loop); }
  stage.addEventListener('pointermove', function (e) { var r = stage.getBoundingClientRect(); P.tx = e.clientX - r.left; P.ty = e.clientY - r.top; P.target = 1; P.last = now(); wake(); }, { passive: true });
  stage.addEventListener('pointerleave', function () { P.target = 0; P.last = now(); wake(); });
  document.addEventListener('visibilitychange', function () {
    if (document.hidden) pausedAt = now();
    else { if (pausedAt) t0 += now() - pausedAt; pausedAt = 0; wake(); }
  });
  if ('IntersectionObserver' in window) new IntersectionObserver(function (en) {
    var v = en[0].isIntersecting;
    if (!v && visible) pausedAt = now(); else if (v && !visible && pausedAt) { t0 += now() - pausedAt; pausedAt = 0; }
    visible = v; if (v) wake();
  }).observe(stage);
  if (replayBtn) { replayBtn.hidden = false; replayBtn.addEventListener('click', play); }
  document.addEventListener('dctx:motion', play);
  document.addEventListener('dctx:theme', function () { colors(); if (!raf) render(motion() ? Math.min((now() - t0) / 1000, 13) : 13, 0); });
  var ro = 'ResizeObserver' in window ? new ResizeObserver(function () { size(); if (!raf) render(done || !motion() ? 13 : Math.min((now() - t0) / 1000, 13), 0); }) : null;
  colors(); size();
  if (ro) ro.observe(stage); else window.addEventListener('resize', function () { size(); });
  if (window.matchMedia) window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function () { colors(); if (!raf) render(13, 0); });
  // Start after the first paint and the load event, when the main thread is idle: the poster is
  // already on screen, so the animation never delays content.
  function boot() { (window.requestIdleCallback || function (f) { setTimeout(f, 120); })(function () { requestAnimationFrame(play); }, { timeout: 900 }); }
  if (document.readyState === 'complete') boot(); else window.addEventListener('load', boot);
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
  function kit(ctx) {
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
    g.check = function (x, y, s, col, a, p) { // a tick drawn along its stroke, p = 0..1
      if (a <= .01 || p <= 0) return;
      var pts = [[x - s, y], [x - s * .35, y + s * .65], [x + s, y - s * .75]], l1 = .38, q = Math.min(1, p / l1);
      ctx.globalAlpha = a; ctx.strokeStyle = C[col]; ctx.lineWidth = Math.max(1.5, s / 3.5); ctx.lineCap = 'round'; ctx.lineJoin = 'round';
      ctx.beginPath(); ctx.moveTo(pts[0][0], pts[0][1]); ctx.lineTo(lerp(pts[0][0], pts[1][0], q), lerp(pts[0][1], pts[1][1], q));
      if (p > l1) { q = (p - l1) / (1 - l1); ctx.lineTo(lerp(pts[1][0], pts[2][0], q), lerp(pts[1][1], pts[2][1], q)); }
      ctx.stroke();
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
            [7.6, 'dctx doctor would list both (planned)']],
    draw: function (g, t, D) {
      var V = D.v || '1.2.0', P = D.pv || '1.1.0', a0 = sm(0, .6, t), p3 = sm(4.2, 5.2, t), p4 = sm(5.8, 6.8, t), p5 = sm(7.6, 8.2, t);
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
      g.tx('↑ called by agents and MCP', 134, 202, 'ink', p4, 9, 400, 'center');
      g.tx('↑ called by app code and CI', 386, 202, 'ink', p4, 9, 400, 'center');
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
        ["# download your platform's archive from GitHub Releases", '$ sha256sum -c SHA256SUMS --ignore-missing', '$ dctx --version', 'dctx ' + (D.v || '1.2.0')],
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
    var g = kit(ctx), stage = cv.parentNode, stepEl = fig.querySelector('[data-step]'), ctl = fig.querySelector('[data-ctl]');
    var pb = fig.querySelector('[data-pause]'), pl = pb && pb.querySelector('[data-l]'), rb = fig.querySelector('[data-replay]');
    var N = Math.round(S.t * FPS), f = 0, paused = false, seen = !('IntersectionObserver' in window), raf = 0, sc = 1, DPR = 1, last = -1;
    function size() {
      var w = Math.max(1, stage.getBoundingClientRect().width);
      DPR = Math.min(2, window.devicePixelRatio || 1); sc = w / W0;
      cv.width = Math.round(w * DPR); cv.height = Math.round(w * H0 / W0 * DPR);
    }
    function render() {
      var t = f / FPS, i = 0;
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
