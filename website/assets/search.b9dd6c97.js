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
