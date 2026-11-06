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
