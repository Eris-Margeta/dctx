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

/* Client-side search over /assets/search-index.*.json (titles, headings, excerpts). */
(function () {
  'use strict';
  var form = document.getElementById('search-form'); if (!form || !window.fetch) return;
  var input = form.elements.q, list = document.getElementById('search-results'), note = document.getElementById('search-note'), index = null;
  function esc(s) { return s.replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); }
  function run() {
    var q = input.value.trim().toLowerCase(), terms = q.split(/\s+/).filter(Boolean);
    if (!index || !terms.length) { list.innerHTML = ''; note.textContent = index ? '' : 'Loading the index…'; return; }
    var hits = index.map(function (p) {
      var t = p.title.toLowerCase(), h = p.h.join(' ').toLowerCase(), x = p.text.toLowerCase(), score = 0;
      for (var i = 0; i < terms.length; i++) {
        var w = terms[i], s = (t.indexOf(w) >= 0 ? 10 : 0) + (h.indexOf(w) >= 0 ? 4 : 0) + (x.indexOf(w) >= 0 ? 1 : 0);
        if (!s) return null; score += s;
      }
      return { p: p, s: score };
    }).filter(Boolean).sort(function (a, b) { return b.s - a.s || a.p.url.localeCompare(b.p.url); }).slice(0, 30);
    note.textContent = hits.length + ' page' + (hits.length === 1 ? '' : 's') + ' match “' + input.value.trim() + '”.';
    list.innerHTML = hits.map(function (r) {
      var heads = r.p.h.filter(function (h) { return terms.some(function (w) { return h.toLowerCase().indexOf(w) >= 0; }); }).slice(0, 3);
      return '<li><a href="' + esc(r.p.url) + '">' + esc(r.p.title) + '</a><div class="u">' + esc(r.p.url) + '</div><p>' + esc(heads.length ? heads.join(' · ') : r.p.text) + '</p></li>';
    }).join('');
  }
  fetch(form.getAttribute('data-index')).then(function (r) { return r.json(); }).then(function (j) { index = j; run(); }, function () { note.textContent = 'The search index could not be loaded.'; });
  var q = new URLSearchParams(location.search).get('q'); if (q) input.value = q;
  input.addEventListener('input', run);
  form.addEventListener('submit', function (e) { e.preventDefault(); history.replaceState(null, '', '?q=' + encodeURIComponent(input.value)); run(); });
})();
