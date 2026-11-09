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
  if (menu) document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && menu.open) { menu.open = false; menu.querySelector('summary').focus(); } });

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
