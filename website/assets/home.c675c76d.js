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
