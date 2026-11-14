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

/* Playground: sends the form to the JSON API and shows the answer in a terminal pane.
   Without JavaScript the same form posts to /api/v1/run-form and the server answers with a page. */
(function () {
  'use strict';
  var form = document.getElementById('pg-form'); if (!form || !window.fetch) return;
  var out = document.getElementById('pg-out'), pre = out.querySelector('pre'), title = out.querySelector('.title');
  var status = document.getElementById('pg-status'), offline = document.getElementById('pg-offline');
  var cmd = form.elements.cmd, files = form.elements.files, sample = form.elements.sample;
  function esc(s) { return s.replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); }
  function say(text, kind) { status.textContent = text; status.className = 'status' + (kind ? ' ' + kind : ''); }
  function split(line) {
    var words = [], w = '', inw = false, i = 0, c, q;
    for (; i < line.length; i++) {
      c = line[i];
      if (c === "'" || c === '"') { q = c; inw = true; for (i++; i < line.length && line[i] !== q; i++) { if (q === '"' && line[i] === '\\' && (line[i + 1] === '"' || line[i + 1] === '\\')) i++; w += line[i]; } if (i >= line.length) throw new Error('unclosed ' + q + ' quote'); }
      else if (c === '\\' && i + 1 < line.length) { w += line[++i]; inw = true; }
      else if (/\s/.test(c)) { if (inw) { words.push(w); w = ''; inw = false; } }
      else { w += c; inw = true; }
    }
    if (inw) words.push(w);
    if (words[0] === 'dctx') words.shift();
    return words;
  }
  function parseFiles(text) {
    var list = [], cur = null;
    text.replace(/\r\n/g, '\n').split(/(?<=\n)/).forEach(function (line) {
      var m = /^--- (.+) ---\n?$/.exec(line);
      if (m) { cur = { path: m[1].trim(), content: '' }; list.push(cur); }
      else if (cur) cur.content += line;
      else if (line.trim()) throw new Error('start each file with a line --- path/to/file ---');
    });
    return list;
  }
  function source() { var r = form.querySelector('input[name="source"]:checked'); return r ? r.value : 'sample'; }
  function line(cls, text) { return '<span class="ln ' + cls + '">' + esc(text) + '</span>'; }
  function show(command, res) {
    var html = '<span class="ln cmd"><span class="p">$ </span>' + esc(command) + '</span>';
    (res.stdout || '').replace(/\n$/, '').split('\n').forEach(function (l) { if (l !== '' || res.stdout) html += line(/^#/.test(l) ? 'out hdrl' : 'out', l); });
    if (res.stderr) res.stderr.replace(/\n$/, '').split('\n').forEach(function (l) { html += line('err', l); });
    var ex = res.exit === null ? 'killed (' + res.killed + ')' : 'exit ' + res.exit;
    html += '<span class="ln meta"># <b' + (res.exit === 0 ? '' : ' class="bad"') + '>' + ex + '</b> · ' + res.bytes + ' bytes · ~' + res.est_tokens + ' est. tokens · ' + res.ms + ' ms' + (res.truncated ? ' · truncated' : '') + '</span>';
    pre.innerHTML = html; title.textContent = 'dctx ' + command; out.querySelector('.scroll').scrollTop = 0;
  }
  function post(url, body) {
    return fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then(function (r) {
      var type = r.headers.get('Content-Type') || '';
      if (type.indexOf('application/json') < 0) { var e = new Error('offline'); e.offline = true; throw e; }
      return r.json().then(function (j) { if (!r.ok) { var err = new Error(j.reason || ('HTTP ' + r.status)); err.status = r.status; throw err; } return j; });
    }, function () { var e = new Error('offline'); e.offline = true; throw e; });
  }
  function fail(e) {
    if (e.offline) { offline.hidden = false; say('The playground service is offline. The recorded runs below are unchanged.', 'bad'); }
    else say((e.status === 429 ? 'Rate limit: ' : e.status === 503 ? 'Busy: ' : 'Refused: ') + e.message, 'bad');
  }
  function place(body) {
    if (source() === 'files') body.files = parseFiles(files.value); else body.sample = sample.value;
    return body;
  }
  form.addEventListener('submit', function (ev) {
    ev.preventDefault();
    var argv, body;
    try { argv = split(cmd.value); body = place({ cmd: argv }); } catch (e) { say(e.message, 'bad'); return; }
    say('Running…');
    post('/api/v1/run', body).then(function (res) { show(argv.join(' '), res); say('Done. ' + (res.exit === 0 ? 'exit 0' : 'exit ' + res.exit) + ' in ' + res.ms + ' ms.', res.exit === 0 ? 'good' : ''); }, fail);
  });
  [].slice.call(document.querySelectorAll('[data-cmd]')).forEach(function (b) {
    b.addEventListener('click', function () { cmd.value = b.getAttribute('data-cmd'); form.requestSubmit ? form.requestSubmit() : form.dispatchEvent(new Event('submit')); });
  });
  fetch('/api/v1/health').then(function (r) { return r.ok ? r.json() : Promise.reject(); }).then(function (h) {
    say('Online: dctx ' + h.dctx + ' (' + h.sha + '), ' + h.samples + ' samples, ' + h.limits.requests_per_minute + ' runs per minute.', 'good');
  }, function () { offline.hidden = false; say('The playground service is offline. The recorded runs below were captured at build time.', 'bad'); });
})();
