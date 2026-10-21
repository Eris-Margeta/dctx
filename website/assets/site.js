/* dctx.app — interaction and motion. No dependencies.
   Content is complete without this file; it only adds motion and conveniences. */
(function () {
  'use strict';
  window.__dctxMotion = true;
  var root = document.documentElement;
  var CANONICAL = 'https://dctx.app/';
  var STORE = 'dctx-motion';
  var fine = window.matchMedia('(hover: hover) and (pointer: fine)').matches;
  var motion = root.classList.contains('motion');
  var $ = function (s, c) { return (c || document).querySelector(s); };
  var $$ = function (s, c) { return Array.prototype.slice.call((c || document).querySelectorAll(s)); };
  var clamp = function (v, a, b) { return Math.max(a, Math.min(b, v)); };
  var smooth = function (a, b, x) { var t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };
  var lerp = function (a, b, t) { return a + (b - a) * t; };
  var fmt = function (n) { return Math.round(n).toLocaleString('en-US'); };
  var now = function () { return performance.now(); };

  /* ---------- Header: border, progress, mobile menu, nav pill ---------- */
  var header = $('.site-header'), progress = $('.progress');
  var toggle = $('.menu-toggle'), nav = $('.nav');
  toggle.addEventListener('click', function () {
    var open = nav.classList.toggle('open');
    toggle.setAttribute('aria-expanded', String(open));
    toggle.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
  });
  $$('.nav-links a').forEach(function (a) {
    a.addEventListener('click', function () { nav.classList.remove('open'); toggle.setAttribute('aria-expanded', 'false'); });
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && nav.classList.contains('open')) { nav.classList.remove('open'); toggle.setAttribute('aria-expanded', 'false'); toggle.focus(); }
  });
  var pill = $('.nav-pill'), links = $('.nav-links');
  if (pill && fine) {
    links.classList.add('has-pill'); nav.classList.add('has-pill');
    var place = function (a) {
      var r = a.getBoundingClientRect(), lr = links.getBoundingClientRect();
      pill.style.width = r.width + 'px'; pill.style.transform = 'translateX(' + (r.left - lr.left) + 'px)'; pill.style.opacity = '1';
    };
    $$('.nav-links a').forEach(function (a) { a.addEventListener('mouseenter', function () { place(a); }); a.addEventListener('focus', function () { place(a); }); });
    links.addEventListener('mouseleave', function () { pill.style.opacity = '0'; });
  }

  /* ---------- Split headlines into masked words ---------- */
  function split(el) {
    var i = 0;
    (function walk(node) {
      Array.prototype.slice.call(node.childNodes).forEach(function (child) {
        if (child.nodeType === 3) {
          var parts = child.textContent.split(/( +)/), frag = document.createDocumentFragment();
          parts.forEach(function (p) {
            if (!p) return;
            if (/^ +$/.test(p)) { frag.appendChild(document.createTextNode(' ')); return; }
            var w = document.createElement('span'); w.className = 'w';
            var wi = document.createElement('span'); wi.className = 'wi'; wi.style.setProperty('--i', i++); wi.textContent = p;
            w.appendChild(wi); frag.appendChild(w);
          });
          child.parentNode.replaceChild(frag, child);
        } else if (child.nodeType === 1 && child.tagName !== 'BR') { walk(child); }
      });
    })(el);
    el.classList.add('split');
  }
  if (motion) $$('h1, .section h2, .compiler h2, .cta h2').forEach(split);

  /* ---------- Quote: words light up as you read ---------- */
  var quote = $('.quote'), qwords = [];
  if (quote && motion) {
    (function wrap(node) {
      Array.prototype.slice.call(node.childNodes).forEach(function (child) {
        if (child.nodeType === 3) {
          var frag = document.createDocumentFragment();
          child.textContent.split(/(\s+)/).forEach(function (p) {
            if (!p) return;
            if (/^\s+$/.test(p)) { frag.appendChild(document.createTextNode(p)); return; }
            var s = document.createElement('span'); s.className = 'qw'; s.textContent = p; frag.appendChild(s);
          });
          child.parentNode.replaceChild(frag, child);
        } else if (child.nodeType === 1) { wrap(child); }
      });
    })(quote);
    qwords = $$('.qw', quote);
  }

  /* ---------- Stagger indices ---------- */
  $$('[data-stagger], [data-anim="depth"]').forEach(function (g) { Array.prototype.forEach.call(g.children, function (c, i) { c.style.setProperty('--i', i); }); });
  $$('.table-wrap').forEach(function (t) { $$('tbody tr', t).forEach(function (tr, i) { tr.style.setProperty('--i', i); $$('.pill', tr).forEach(function (p) { p.style.setProperty('--i', i); }); }); });
  $$('.layers').forEach(function (l) { $$('li', l).forEach(function (li, i) { li.style.setProperty('--i', i); }); });
  $$('.step').forEach(function (s, i) { s.style.setProperty('--i', i); });
  $$('.era').forEach(function (s, i) { s.style.setProperty('--i', i); });
  $$('.hero .trust li').forEach(function (s, i) { s.style.setProperty('--i', i); });
  $$('.code').forEach(function (c) { $$('.ln', c).forEach(function (l, i) { var n = Math.max(1, l.textContent.length); l.style.setProperty('--i', i); l.style.setProperty('--n', n); l.style.setProperty('--t', Math.min(900, 18 * n) + 'ms'); }); });
  $$('.draw').forEach(function (svg) { $$('path, circle, rect, line', svg).forEach(function (p) { p.setAttribute('pathLength', '1'); }); });
  $$('.hero .trust svg path').forEach(function (p) { p.setAttribute('pathLength', '1'); });

  /* ---------- Counters and decoders ---------- */
  function count(el) {
    var to = parseFloat(el.getAttribute('data-count')), dur = +(el.getAttribute('data-dur') || 1600), t0 = now();
    if (!motion) { el.textContent = fmt(to); return; }
    (function tick(t) {
      if (!motion) { el.textContent = fmt(to); return; }
      var k = clamp((t - t0) / dur, 0, 1), e = 1 - Math.pow(1 - k, 4);
      el.textContent = fmt(to * e);
      if (k < 1) requestAnimationFrame(tick);
    })(t0);
  }
  var GLYPHS = '0123456789ABCDEF#%&*+<>';
  function decode(el) {
    var final = el.getAttribute('data-decode') || el.textContent, t0 = now(), dur = 900;
    if (!motion) { el.textContent = final; return; }
    (function tick(t) {
      if (!motion) { el.textContent = final; return; }
      var k = clamp((t - t0) / dur, 0, 1), out = '';
      for (var i = 0; i < final.length; i++) out += (k * final.length > i + 0.5 || final[i] === ' ') ? final[i] : GLYPHS[(Math.random() * GLYPHS.length) | 0];
      el.textContent = out;
      if (k < 1) requestAnimationFrame(tick); else el.textContent = final;
    })(t0);
  }
  $$('[data-decode]').forEach(function (el) { el.setAttribute('aria-label', el.getAttribute('data-decode')); });

  /* ---------- Reveal on scroll ---------- */
  var revealables = $$('.reveal, [data-stagger], [data-anim], .split, .spectrum, .table-wrap, .layers, .code, .timeline, .facts, .eyebrow, .draw');
  var spectrum = $('.spectrum'), spectrumAt = 0;
  function onReveal(el) {
    if (el.classList.contains('in')) return;
    el.classList.add('in');
    if (el === spectrum) spectrumAt = now();
    $$('[data-count]', el).forEach(count);
    if (el.matches('[data-count]')) count(el);
    $$('[data-decode]', el).forEach(function (d, i) { setTimeout(function () { decode(d); }, motion ? 350 + i * 350 : 0); });
  }
  var io = null;
  if (!motion || !('IntersectionObserver' in window)) { revealables.forEach(onReveal); }
  else {
    io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { if (en.isIntersecting) { onReveal(en.target); io.unobserve(en.target); } });
    }, { rootMargin: '0px 0px -12% 0px', threshold: 0.12 });
    revealables.forEach(function (el) { if (!el.closest('.hero')) io.observe(el); });
  }

  /* ---------- Hero choreography: headline, then a live terminal session ---------- */
  var hero = $('.hero'), term = $('#term'), heroTimers = [];
  function heroFinal() {
    heroTimers.forEach(clearTimeout); heroTimers = [];
    $$('.ln', term).forEach(function (l) { l.classList.remove('typing'); l.classList.add('on'); $$('[data-count]', l).forEach(function (c) { c.textContent = fmt(parseFloat(c.getAttribute('data-count'))); }); });
    $$('.chip-float').forEach(function (c) { c.classList.add('on'); });
    var lines = $$('.ln', term); if (lines.length) lines[lines.length - 1].classList.add('caret');
  }
  function heroGo() {
    hero.classList.add('ready');
    $$('.split', hero).forEach(function (h) { h.classList.add('in'); });
    if (!motion) { heroFinal(); return; }
    var lines = $$('.ln', term), t = 1000;
    lines.forEach(function (l) {
      var isCmd = l.classList.contains('cmd');
      if (isCmd) {
        var n = l.textContent.length, d = Math.round(n * 34);
        heroTimers.push(setTimeout(function () { l.style.setProperty('--n', n); l.style.setProperty('--t', d + 'ms'); l.classList.add('typing'); }, t));
        t += d + 260;
        heroTimers.push(setTimeout(function () { l.classList.remove('typing'); l.classList.add('on'); }, t - 120));
      } else {
        heroTimers.push(setTimeout(function () { l.classList.add('on'); $$('[data-count]', l).forEach(count); }, t));
        t += l.classList.contains('rec') ? 110 : 200;
      }
    });
    heroTimers.push(setTimeout(function () { $('.chip-2').classList.add('on'); }, 1500));
    heroTimers.push(setTimeout(function () { $('.chip-1').classList.add('on'); var last = lines[lines.length - 1]; last.classList.add('caret'); }, t + 200));
  }
  setTimeout(heroGo, 60);

  /* ---------- Hero vector field: a flow of fine strokes that swirls around the pointer ---------- */
  var field = null;
  (function () {
    var cv = $('#hero-field'); if (!cv || !cv.getContext) return;
    var ctx = cv.getContext('2d'); if (!ctx) return;
    var W = 0, H = 0, DPR = 1, N = 0, px, py, calm, seedv, textCol = null;
    // Deterministic gradient noise
    var perm = new Uint8Array(512);
    (function () { var p = [], i, s = 1337; for (i = 0; i < 256; i++) p[i] = i; for (i = 255; i > 0; i--) { s = (s * 1664525 + 1013904223) >>> 0; var j = (s / 4294967296 * (i + 1)) | 0, t = p[i]; p[i] = p[j]; p[j] = t; } for (i = 0; i < 512; i++) perm[i] = p[i & 255]; })();
    function fade(t) { return t * t * t * (t * (t * 6 - 15) + 10); }
    function grad(h, x, y) { switch (h & 7) { case 0: return x + y; case 1: return -x + y; case 2: return x - y; case 3: return -x - y; case 4: return x; case 5: return -x; case 6: return y; default: return -y; } }
    function noise(x, y) {
      var X = Math.floor(x), Y = Math.floor(y); x -= X; y -= Y; X &= 255; Y &= 255;
      var u = fade(x), v = fade(y), A = perm[X] + Y, B = perm[X + 1] + Y;
      return lerp(lerp(grad(perm[A], x, y), grad(perm[B], x - 1, y), u), lerp(grad(perm[A + 1], x, y - 1), grad(perm[B + 1], x - 1, y - 1), u), v);
    }
    var rnd = (function (s) { return function () { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; }; })(7);
    // Pointer state with inertia: a fast follower and a lagging one (the wake)
    var P = { tx: 0, ty: 0, x: 0, y: 0, lx: 0, ly: 0, has: false, pres: 0, target: 0, speed: 0, vx: 0, vy: 0, strength: 1, last: 0 };
    var vis = true, sized = false;
    function size() {
      var r = hero.getBoundingClientRect(); DPR = Math.min(2, window.devicePixelRatio || 1);
      W = Math.max(1, Math.round(r.width)); H = Math.max(1, Math.round(r.height));
      cv.width = Math.round(W * DPR); cv.height = Math.round(H * DPR); ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
      var col = $('.hero-grid > div:first-child'), cr = col ? col.getBoundingClientRect() : null;
      textCol = cr ? { l: cr.left - r.left, t: cr.top - r.top, r: cr.right - r.left, b: cr.bottom - r.top } : null;
      var sp = clamp(W / 50, 22, 30), cols = Math.ceil(W / sp) + 1, rows = Math.ceil(H / sp) + 1;
      N = cols * rows; px = new Float32Array(N); py = new Float32Array(N); calm = new Float32Array(N); seedv = new Float32Array(N);
      rnd = (function (s) { return function () { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; }; })(7);
      var i = 0;
      for (var yy = 0; yy < rows; yy++) for (var xx = 0; xx < cols; xx++, i++) {
        px[i] = xx * sp + (rnd() - .5) * sp * .5; py[i] = yy * sp + (rnd() - .5) * sp * .5; seedv[i] = rnd();
        var c = 1;
        if (textCol) {
          var soft = 90, dx = Math.max(textCol.l - px[i], px[i] - textCol.r, 0), dy = Math.max(textCol.t - py[i], py[i] - textCol.b, 0), d = Math.sqrt(dx * dx + dy * dy);
          c = lerp(.42, 1, smooth(0, soft, d));
        }
        calm[i] = c;
      }
      if (!P.has) { P.x = P.tx = W * .66; P.y = P.ty = H * .4; P.lx = P.x; P.ly = P.y; }
      sized = true;
    }
    var segs = {}, keys = [];
    var ix = 0, iy = 0, R = 150, R2 = 22500, R2l = 50625, pres = 0, str = 0, idleStr = .5, T = 0;
    // Direction of the field at (x, y): base flow from layered gradient noise, bent by the attractors.
    // Returns the unit vector in out[0..1] and the local energy (0 calm … 1 at the vortex) in out[2].
    var out = [0, 0, 0];
    function dir(x, y) {
      var a = noise(x * .0042 + T * .03, y * .0042 - T * .02) * 3.4 + noise(x * .0013 - T * .012, y * .0013 + T * .01) * 4.6 + .6;
      var vx = Math.cos(a), vy = Math.sin(a), e = 0, dx, dy, d2, d, inf, sx, sy;
      if (pres > .02) {
        dx = P.x - x; dy = P.y - y; d2 = dx * dx + dy * dy;
        if (d2 < R2 * 7) { d = Math.sqrt(d2) || 1; inf = Math.exp(-d2 / (2 * R2)) * str; sx = -dy / d * .9 + dx / d * .38; sy = dx / d * .9 + dy / d * .38; vx += (sx - vx) * inf; vy += (sy - vy) * inf; if (inf > e) e = inf; }
        dx = P.lx - x; dy = P.ly - y; d2 = dx * dx + dy * dy;
        if (d2 < R2l * 5) { d = Math.sqrt(d2) || 1; inf = Math.exp(-d2 / (2 * R2l)) * str * .45; sx = -dy / d; sy = dx / d; vx += (sx - vx) * inf; vy += (sy - vy) * inf; if (inf > e) e = inf; }
      }
      dx = ix - x; dy = iy - y; d2 = dx * dx + dy * dy;
      if (d2 < R2l * 5) { d = Math.sqrt(d2) || 1; inf = Math.exp(-d2 / (2 * R2l)) * idleStr; sx = -dy / d * .92 + dx / d * .3; sy = dx / d * .92 + dy / d * .3; vx += (sx - vx) * inf; vy += (sy - vy) * inf; if (inf * .75 > e) e = inf * .75; }
      var len = Math.sqrt(vx * vx + vy * vy) || 1; out[0] = vx / len; out[1] = vy / len; out[2] = e;
      return out;
    }
    function draw(time, still) {
      if (!sized) return;
      T = time * 0.001;
      ctx.clearRect(0, 0, W, H);
      if (!still) {
        var ox = P.x, oy = P.y;
        P.x += (P.tx - P.x) * .14; P.y += (P.ty - P.y) * .14;
        P.lx += (P.x - P.lx) * .05; P.ly += (P.y - P.ly) * .05;
        var sp = Math.sqrt((P.x - ox) * (P.x - ox) + (P.y - oy) * (P.y - oy));
        P.speed += (clamp(sp / 18, 0, 1) - P.speed) * .1;
        P.pres += (P.target - P.pres) * (P.target > P.pres ? .08 : .03);
      }
      // the idle attractor wanders slowly through the open part of the hero
      ix = W * .64 + Math.cos(T * .21) * W * .2 + Math.sin(T * .09) * W * .08; iy = H * .5 + Math.sin(T * .17) * H * .26 + Math.cos(T * .07) * H * .1;
      pres = still ? 0 : P.pres; R = clamp(W * .12, 120, 190); R2 = R * R; R2l = (R * 1.5) * (R * 1.5);
      str = pres * P.strength * (.72 + .28 * P.speed); idleStr = still ? .55 : .5 * (1 - pres * .6);
      if (pres > .02) {
        var g = ctx.createRadialGradient(P.x, P.y, 0, P.x, P.y, R * 1.9);
        g.addColorStop(0, 'rgba(106,68,240,' + (.11 * pres).toFixed(3) + ')'); g.addColorStop(.5, 'rgba(106,68,240,' + (.04 * pres).toFixed(3) + ')'); g.addColorStop(1, 'rgba(106,68,240,0)');
        ctx.fillStyle = g; ctx.fillRect(P.x - R * 2, P.y - R * 2, R * 4, R * 4);
      }
      for (var k = 0; k < keys.length; k++) segs[keys[k]].length = 0;
      for (var i = 0; i < N; i++) {
        var x = px[i], y = py[i], o = dir(x, y), e = o[2];
        var am = .5 + .5 * noise(x * .0024 + T * .05, y * .0024 - T * .035);
        var c = calm[i], alpha = (.07 + .08 * am) * c * (W < 640 ? .8 : 1); alpha = lerp(alpha, .8 * lerp(.5, 1, c), e);
        var L = (10 + 7 * am + 22 * e) * lerp(.7, 1, c) * (.8 + .4 * seedv[i]), h = L / 4;
        // trace a short flow line through the point: two steps back, two steps forward
        var vx = o[0], vy = o[1];
        var f1x = x + vx * h, f1y = y + vy * h; o = dir(f1x, f1y); var f2x = f1x + o[0] * h, f2y = f1y + o[1] * h;
        var b1x = x - vx * h, b1y = y - vy * h; o = dir(b1x, b1y); var b2x = b1x - o[0] * h, b2y = b1y - o[1] * h;
        var ei = Math.round(e * 10), ai = Math.round(alpha * 24), key = ei * 32 + ai;
        var arr = segs[key]; if (!arr) { arr = segs[key] = []; keys.push(key); keys.sort(function (p, q) { return p - q; }); }
        arr.push(b2x, b2y, b1x, b1y, x, y, f1x, f1y, f2x, f2y);
      }
      ctx.lineCap = 'round'; ctx.lineJoin = 'round';
      for (k = 0; k < keys.length; k++) {
        var kk = keys[k], arr2 = segs[kk]; if (!arr2.length) continue;
        var eh = (kk >> 5) / 10, ah = (kk & 31) / 24;
        ctx.strokeStyle = 'rgba(' + Math.round(lerp(17, 106, eh)) + ',' + Math.round(lerp(19, 68, eh)) + ',' + Math.round(lerp(24, 240, eh)) + ',' + ah.toFixed(3) + ')';
        ctx.lineWidth = 1 + eh * .8;
        ctx.beginPath();
        for (var j = 0; j < arr2.length; j += 10) { ctx.moveTo(arr2[j], arr2[j + 1]); ctx.lineTo(arr2[j + 2], arr2[j + 3]); ctx.lineTo(arr2[j + 4], arr2[j + 5]); ctx.lineTo(arr2[j + 6], arr2[j + 7]); ctx.lineTo(arr2[j + 8], arr2[j + 9]); }
        ctx.stroke();
      }
    }
    hero.addEventListener('pointermove', function (e) {
      var r = hero.getBoundingClientRect(); P.tx = e.clientX - r.left; P.ty = e.clientY - r.top; P.has = true; P.target = 1; P.strength = e.pointerType === 'touch' ? .6 : 1; P.last = now();
    }, { passive: true });
    hero.addEventListener('pointerdown', function (e) { var r = hero.getBoundingClientRect(); P.tx = e.clientX - r.left; P.ty = e.clientY - r.top; P.has = true; P.target = 1; P.strength = e.pointerType === 'touch' ? .6 : 1; }, { passive: true });
    hero.addEventListener('pointerleave', function () { P.target = 0; });
    hero.addEventListener('pointerup', function (e) { if (e.pointerType === 'touch') P.target = 0; }, { passive: true });
    hero.addEventListener('pointercancel', function () { P.target = 0; });
    var ro = ('ResizeObserver' in window) ? new ResizeObserver(function () { size(); if (!motion) draw(0, true); }) : null;
    if (ro) ro.observe(hero); else window.addEventListener('resize', function () { size(); if (!motion) draw(0, true); });
    size();
    if ('IntersectionObserver' in window) new IntersectionObserver(function (en) { vis = en[0].isIntersecting; }, { rootMargin: '80px 0px' }).observe(hero);
    field = { draw: draw, visible: function () { return vis; }, still: function () { draw(0, true); } };
    if (!motion) field.still();
  })();

  /* ---------- Pointer: hero parallax, spotlights, tilt, magnetic buttons, lights ---------- */
  if (fine) {
    var hx = 0, hy = 0, hpending = false;
    hero.addEventListener('pointermove', function (e) {
      if (!motion) return;
      var r = hero.getBoundingClientRect(); hx = (e.clientX - r.left) / r.width - .5; hy = (e.clientY - r.top) / r.height - .5;
      if (!hpending) { hpending = true; requestAnimationFrame(function () { hero.style.setProperty('--px', hx.toFixed(3)); hero.style.setProperty('--py', hy.toFixed(3)); hpending = false; }); }
    });
    hero.addEventListener('pointerleave', function () { hero.style.setProperty('--px', 0); hero.style.setProperty('--py', 0); });

    $$('.spot').forEach(function (c) {
      c.addEventListener('pointermove', function (e) { var r = c.getBoundingClientRect(); c.style.setProperty('--mx', (e.clientX - r.left) + 'px'); c.style.setProperty('--my', (e.clientY - r.top) + 'px'); });
    });
    $$('.tilt').forEach(function (c) {
      c.addEventListener('pointermove', function (e) {
        if (!motion) return;
        var r = c.getBoundingClientRect(), x = (e.clientX - r.left) / r.width - .5, y = (e.clientY - r.top) / r.height - .5;
        c.style.transform = 'perspective(900px) rotateX(' + (-y * 4).toFixed(2) + 'deg) rotateY(' + (x * 5).toFixed(2) + 'deg) translateY(-3px)';
      });
      c.addEventListener('pointerleave', function () { c.style.transform = ''; });
    });
    $$('.btn').forEach(function (b) {
      b.classList.add('magnetic');
      b.addEventListener('pointermove', function (e) { if (!motion) return; var r = b.getBoundingClientRect(); b.style.transform = 'translate(' + ((e.clientX - r.left - r.width / 2) * .12).toFixed(1) + 'px,' + ((e.clientY - r.top - r.height / 2) * .22).toFixed(1) + 'px)'; });
      b.addEventListener('pointerleave', function () { b.style.transform = ''; });
    });
    var cta = $('.cta'), glow = $('.cta .glow');
    if (cta && glow) cta.addEventListener('pointermove', function (e) { var r = cta.getBoundingClientRect(); cta.style.setProperty('--gx', (e.clientX - r.left) + 'px'); cta.style.setProperty('--gy', (e.clientY - r.top) + 'px'); });
    $$('.section.dark').forEach(function (s) {
      var pending = false, lx = 0, ly = 0;
      s.addEventListener('pointermove', function (e) {
        var r = s.getBoundingClientRect(); lx = e.clientX - r.left; ly = e.clientY - r.top;
        if (!pending) { pending = true; requestAnimationFrame(function () { s.style.setProperty('--lx', lx + 'px'); s.style.setProperty('--ly', ly + 'px'); s.style.setProperty('--lo', '1'); pending = false; }); }
      });
      s.addEventListener('pointerleave', function () { s.style.setProperty('--lo', '0'); });
    });
    $$('.layers li').forEach(function (li) {
      li.addEventListener('mouseenter', function () { var k = li.getAttribute('data-key'); if (!k) return; $$('.code .ln[data-key="' + k + '"]').forEach(function (l) { l.classList.add('hl'); }); });
      li.addEventListener('mouseleave', function () { $$('.code .ln.hl').forEach(function (l) { l.classList.remove('hl'); }); });
    });
  }

  /* ---------- FAQ: animated disclosure ---------- */
  $$('.faq details').forEach(function (d) {
    var s = $('summary', d), body = $('.faq-body', d);
    if (!body) return;
    s.addEventListener('click', function (e) {
      if (!motion || !body.animate) return;
      e.preventDefault();
      if (d.open) {
        var h = body.offsetHeight;
        body.animate([{ height: h + 'px', opacity: 1 }, { height: '0px', opacity: 0 }], { duration: 380, easing: 'cubic-bezier(.16,1,.3,1)' }).onfinish = function () { d.open = false; };
      } else {
        d.open = true; var h2 = body.offsetHeight;
        body.animate([{ height: '0px', opacity: 0 }, { height: h2 + 'px', opacity: 1 }], { duration: 520, easing: 'cubic-bezier(.16,1,.3,1)' });
      }
    });
  });

  /* ---------- The prompt compiler (scroll-driven canvas) ---------- */
  var comp = $('#compiler'), canvas = $('#compiler-canvas');
  var C = null;
  if (comp && canvas && canvas.getContext) {
    var ctx = canvas.getContext('2d'), W = 0, H = 0, DPR = 1, parts = [], target = [], seeded = false;
    var phases = $$('.phases li', comp), mIn = $('#meter-in'), mInLabel = $('#meter-in-label'), mOut = $('#meter-out');
    var rnd = (function (s) { return function () { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; }; })(20251115);
    function gauss() { return (rnd() + rnd() + rnd() + rnd() - 2) / 2; }
    function size() {
      var r = canvas.getBoundingClientRect(); DPR = Math.min(2, window.devicePixelRatio || 1);
      W = Math.max(1, r.width); H = Math.max(1, r.height);
      canvas.width = Math.round(W * DPR); canvas.height = Math.round(H * DPR); ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
      seed();
    }
    function seed() {
      rnd = (function (s) { return function () { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296; }; })(20251115);
      var N = W < 560 ? 650 : 1500, cx = W * 0.36, cy = H * 0.46, sx = W * 0.26, sy = H * 0.3;
      parts = []; target = [];
      var cols = W < 560 ? 6 : 8, bw = W < 560 ? 26 : 34, bh = 7, gx = 6, gy = 7, rows = 0, relevant = 0;
      for (var i = 0; i < N; i++) {
        var u = rnd(), kind = u < 0.32 ? 0 : (u < 0.955 ? 1 : 2);
        var x = cx + gauss() * sx * 1.6, y = cy + gauss() * sy * 1.6;
        var p = { x0: x, y0: y, w: 3 + rnd() * (kind === 2 ? 10 : 9), h: 3, kind: kind, ph: rnd() * Math.PI * 2, sp: 0.4 + rnd() * 0.8, ang: Math.atan2(y - cy, x - cx), shade: 0.55 + rnd() * 0.45, idx: 0 };
        if (kind === 2) { p.idx = relevant++; }
        parts.push(p);
      }
      rows = Math.ceil(relevant / cols);
      var avail = W < 560 ? W * 0.8 : W * 0.38; bw = clamp((avail - (cols - 1) * gx) / cols, 14, 34);
      var gw = cols * bw + (cols - 1) * gx, gh = rows * bh + (rows - 1) * gy;
      var ox = W < 560 ? (W - gw) / 2 : Math.min(W - 36 - gw, W * 0.75 - gw / 2), oy = H * (W < 560 ? 0.36 : 0.42) - gh / 2;
      parts.forEach(function (p) { if (p.kind === 2) { var c = p.idx % cols, r = (p.idx / cols) | 0; p.tx = ox + c * (bw + gx); p.ty = oy + r * (bh + gy); p.tw = bw; } });
      C.frame = { x: ox - 14, y: oy - 14, w: gw + 28, h: gh + 28, rel: relevant };
      seeded = true;
    }
    C = { frame: null, p: 0, visible: false };
    function draw(p, time) {
      if (!seeded) return;
      ctx.clearRect(0, 0, W, H);
      var s1 = smooth(0.06, 0.34, p), s2 = smooth(0.34, 0.62, p), s3 = smooth(0.62, 0.92, p), idle = (1 - s3) * (motion ? 1 : 0);
      // beam (the compiler)
      if (s2 > 0.01 && W >= 560) {
        var bx = W * 0.6, a = 0.18 * s2 * (1 - s3 * 0.6);
        var g = ctx.createLinearGradient(bx - 30, 0, bx + 30, 0);
        g.addColorStop(0, 'rgba(106,68,240,0)'); g.addColorStop(0.5, 'rgba(106,68,240,' + a + ')'); g.addColorStop(1, 'rgba(106,68,240,0)');
        ctx.fillStyle = g; ctx.fillRect(bx - 30, H * 0.08, 60, H * 0.76);
      }
      for (var i = 0; i < parts.length; i++) {
        var q = parts[i], wob = Math.sin(time * 0.0006 * q.sp + q.ph) * 4 * idle, wob2 = Math.cos(time * 0.0005 * q.sp + q.ph) * 3 * idle;
        var x = q.x0 + wob, y = q.y0 + wob2, w = q.w, alpha, col;
        if (q.kind === 0) {
          x += Math.cos(q.ang) * 90 * s1; y += Math.sin(q.ang) * 90 * s1;
          alpha = (1 - s1) * 0.55 * q.shade; col = '17,19,24';
        } else if (q.kind === 1) {
          alpha = lerp(0.42 * q.shade, 0.12, s2) * (1 - 0.5 * s3); col = '17,19,24';
          w = lerp(q.w, q.w * 0.7, s2);
        } else {
          var stagger = clamp(s3 * 1.35 - (q.idx % 17) * 0.02, 0, 1), e = 1 - Math.pow(1 - stagger, 3);
          x = lerp(x, q.tx, e); y = lerp(y, q.ty, e); w = lerp(q.w, q.tw, e);
          alpha = lerp(0.42 * q.shade, 1, smooth(0.0, 0.6, s2)); col = s2 > 0.05 ? '106,68,240' : '17,19,24';
          if (s2 > 0.05 && s3 < 0.98) { ctx.fillStyle = 'rgba(106,68,240,' + (0.12 * s2 * (1 - e)) + ')'; ctx.fillRect(x - 3, y - 3, w + 6, 9); }
        }
        if (alpha <= 0.004) continue;
        ctx.fillStyle = 'rgba(' + col + ',' + alpha.toFixed(3) + ')';
        ctx.fillRect(x, y, w, q.h + (q.kind === 2 ? lerp(0, 4, s3) : 0));
      }
      var f = C.frame, fa = smooth(0.82, 0.98, p);
      if (f && fa > 0) {
        ctx.save(); ctx.globalAlpha = fa; ctx.strokeStyle = '#6a44f0'; ctx.lineWidth = 1.2; ctx.setLineDash([5, 4]);
        ctx.strokeRect(f.x + 0.5, f.y + 0.5, f.w, f.h); ctx.setLineDash([]);
        ctx.fillStyle = '#4e2fc9'; ctx.font = '600 11px ui-monospace, SFMono-Regular, Menlo, monospace';
        ctx.fillText('≤ BUDGET · STABLE ORDER', f.x, f.y - 8);
        ctx.restore();
      }
    }
    function readout(p) {
      phases.forEach(function (li, i) {
        var on = (i === 0 && p >= 0.04 && p < 0.34) || (i === 1 && p >= 0.34 && p < 0.62) || (i === 2 && p >= 0.62);
        li.classList.toggle('active', on);
      });
      var k = [[0, 1240000], [0.34, 812000], [0.62, 46000], [0.92, 412]], v = 412;
      for (var i = 0; i < k.length - 1; i++) {
        if (p <= k[i + 1][0]) { var t = smooth(k[i][0], k[i + 1][0], p); v = Math.exp(lerp(Math.log(k[i][1]), Math.log(k[i + 1][1]), t)); break; }
      }
      if (mIn) mIn.textContent = fmt(v);
      if (mInLabel) mInLabel.textContent = p < 0.34 ? 'Tokens in the repository' : (p < 0.62 ? 'Authorized by policy' : (p < 0.92 ? 'Ranked, relevant spans' : 'Sent to the model'));
      if (mOut) mOut.textContent = p >= 0.92 ? 'Exact · bounded · stable order' : (p >= 0.62 ? 'Ordering…' : (p >= 0.34 ? 'Ranking…' : 'Selecting…'));
    }
    C.draw = draw; C.readout = readout;
    C.final = function () { C.p = 1; readout(1); draw(1, 0); };
    var ro2 = ('ResizeObserver' in window) ? new ResizeObserver(function () { size(); draw(C.p, now()); }) : null;
    if (ro2) ro2.observe(canvas); else window.addEventListener('resize', function () { size(); });
    size();
    if (!motion) C.final();
    if ('IntersectionObserver' in window) new IntersectionObserver(function (en) { C.visible = en[0].isIntersecting; }, { rootMargin: '200px 0px' }).observe(comp);
  }

  /* ---------- One frame loop for everything scroll-linked ---------- */
  var track = comp ? $('.compiler-track', comp) : null, steps = $$('.step');
  var beliefCol = $('.belief-cards'), beliefCards = beliefCol ? $$('.card', beliefCol) : [];
  var timeline = $('.timeline'), darks = $$('.section.dark'), paras = $$('[data-parallax]');
  var hidden = false;
  document.addEventListener('visibilitychange', function () { hidden = document.hidden; });
  function frame(time) {
    requestAnimationFrame(frame);
    if (hidden) return;
    var y = window.scrollY, vh = window.innerHeight, docH = document.documentElement.scrollHeight - vh;
    header.classList.toggle('scrolled', y > 8);
    if (!motion) return;
    if (progress) progress.style.setProperty('--p', docH > 0 ? (y / docH).toFixed(4) : 0);
    // reads
    var qr = qwords.length ? quote.getBoundingClientRect() : null;
    var tr = (C && C.visible && track) ? track.getBoundingClientRect() : null;
    var br = beliefCol ? beliefCol.getBoundingClientRect() : null;
    var tlr = timeline ? timeline.getBoundingClientRect() : null;
    var drs = darks.map(function (s) { return s.getBoundingClientRect(); });
    var prs = paras.map(function (p) { return p.getBoundingClientRect(); });
    // writes
    if (qr) {
      var k = clamp((vh * 0.85 - qr.top) / (qr.height + vh * 0.35), 0, 1), lit = Math.round(k * qwords.length * 1.15);
      for (var i = 0; i < qwords.length; i++) qwords[i].classList.toggle('lit', i < lit);
    }
    if (tr) {
      var span = tr.height - vh * 0.85, p = clamp((-tr.top + 40) / Math.max(1, span), 0, 1);
      C.p = lerp(C.p, p, 0.18);
      C.readout(C.p); C.draw(C.p, time);
    }
    if (br && br.bottom > 0 && br.top < vh) {
      var rp = clamp((vh * 0.72 - br.top) / Math.max(1, br.height - 36), 0, 1);
      beliefCol.style.setProperty('--rp', rp.toFixed(4));
      var railY = br.top + 36 + rp * (br.height - 72);
      for (i = 0; i < beliefCards.length; i++) { var cr = beliefCards[i].getBoundingClientRect(); beliefCards[i].classList.toggle('lit', railY >= cr.top + 44); }
    }
    if (tlr && tlr.bottom > 0 && tlr.top < vh) {
      timeline.style.setProperty('--tp', clamp((vh * 0.9 - tlr.top) / (vh * 0.55), 0, 1).toFixed(4));
    }
    for (i = 0; i < darks.length; i++) {
      var dr = drs[i]; if (dr.bottom < -100 || dr.top > vh + 100) continue;
      darks[i].style.setProperty('--bloom', smooth(vh * 1.05, vh * 0.35, dr.top).toFixed(3));
    }
    for (i = 0; i < paras.length; i++) {
      var pr = prs[i]; if (pr.bottom < -200 || pr.top > vh + 200) continue;
      var f = parseFloat(paras[i].getAttribute('data-parallax')) || 0, off = (pr.top + pr.height / 2 - vh / 2) * f;
      paras[i].style.transform = 'translate3d(0,' + off.toFixed(1) + 'px,0)';
    }
    if (spectrumAt && steps.length && window.innerWidth > 920) {
      var ph = (time - spectrumAt - 2200) / 3600, idx = -1;
      if (ph >= 0) { var pos = (ph % 1) * 4, near = Math.round(pos); if (Math.abs(pos - near) < .28) idx = near; }
      for (i = 0; i < steps.length; i++) steps[i].classList.toggle('lit', i === idx);
    }
    if (field && field.visible()) field.draw(time, false);
  }
  requestAnimationFrame(frame);

  /* ---------- Motion toggle: a visible switch, remembered on this device ---------- */
  var mt = $('#motion-toggle');
  function paintToggle() { if (mt) mt.setAttribute('aria-pressed', String(!motion)); }
  function finishAll() {
    revealables.forEach(onReveal);
    $$('[data-count]').forEach(function (c) { c.textContent = fmt(parseFloat(c.getAttribute('data-count'))); });
    $$('[data-decode]').forEach(function (d) { d.textContent = d.getAttribute('data-decode'); });
    heroFinal();
    qwords.forEach(function (w) { w.classList.add('lit'); });
    if (C) C.final();
    if (field) field.still();
    steps.forEach(function (s) { s.classList.remove('lit'); });
    beliefCards.forEach(function (c) { c.classList.add('lit'); });
    paras.forEach(function (p) { p.style.transform = ''; });
    $$('.tilt, .btn').forEach(function (el) { el.style.transform = ''; });
    darks.forEach(function (s) { s.style.setProperty('--bloom', '1'); });
    if (beliefCol) beliefCol.style.setProperty('--rp', '1');
    if (timeline) timeline.style.setProperty('--tp', '1');
    hero.style.setProperty('--px', 0); hero.style.setProperty('--py', 0);
  }
  function setMotion(on) {
    motion = on;
    root.classList.toggle('motion', on);
    try { localStorage.setItem(STORE, on ? 'on' : 'off'); } catch (e) { /* storage unavailable */ }
    if (!on) finishAll();
    else { if (C) C.visible = true; }
    paintToggle();
  }
  if (mt) { paintToggle(); mt.addEventListener('click', function () { setMotion(!motion); }); }

  /* ---------- Heading permalinks and copy-link buttons (web standard) ---------- */
  var toast = $('#toast'), toastTimer;
  function say(html, keep) {
    toast.innerHTML = html; toast.classList.add('show');
    clearTimeout(toastTimer); toastTimer = setTimeout(function () { toast.classList.remove('show'); }, keep ? 9000 : 2200);
  }
  $$('.h-row > h2[id]').forEach(function (h) {
    var id = h.id, name = h.textContent.replace(/\s+/g, ' ').trim().replace(/\.$/, '');
    var url = CANONICAL + '#' + encodeURIComponent(id);
    var tools = document.createElement('span'); tools.className = 'h-tools';
    var a = document.createElement('a'); a.className = 'h-tool'; a.href = '#' + id;
    a.setAttribute('aria-label', 'Link to ' + name); a.dataset.tip = 'Link to section'; a.textContent = '#';
    var b = document.createElement('button'); b.type = 'button'; b.className = 'h-tool';
    b.setAttribute('aria-label', 'Copy link to ' + name); b.dataset.tip = 'Copy link';
    b.innerHTML = '<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2"/><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2"/></svg>';
    b.addEventListener('click', function () {
      function fallback() {
        say('Copy was not possible. Select the link:<input readonly value="' + url + '" aria-label="Link to ' + name + '">', true);
        var inp = toast.querySelector('input'); if (inp) inp.addEventListener('focus', function () { inp.select(); });
      }
      if (!navigator.clipboard || !navigator.clipboard.writeText) { fallback(); return; }
      navigator.clipboard.writeText(url).then(function () { say('Copied link to “' + name + '”'); }, fallback);
    });
    tools.appendChild(a); tools.appendChild(b); h.parentNode.appendChild(tools);
  });
})();
