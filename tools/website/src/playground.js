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
  var tabs = [].slice.call(document.querySelectorAll('.tabs [role="tab"]'));
  tabs.forEach(function (tab) {
    tab.addEventListener('click', function () {
      tabs.forEach(function (t) { var on = t === tab; t.setAttribute('aria-selected', String(on)); document.getElementById(t.getAttribute('aria-controls')).hidden = !on; });
    });
  });
  var mcpBtn = document.getElementById('mcp-run'), mcpIn = document.getElementById('mcp-in');
  if (mcpBtn) mcpBtn.addEventListener('click', function () {
    var reqs;
    try { reqs = mcpIn.value.split('\n').filter(function (l) { return l.trim(); }).map(function (l) { return JSON.parse(l); }); }
    catch (e) { say('Each line must be one JSON-RPC message: ' + e.message, 'bad'); return; }
    say('Running dctx mcp…');
    var body; try { body = place({ requests: reqs }); } catch (e) { say(e.message, 'bad'); return; }
    post('/api/v1/mcp', body).then(function (res) {
      pre.innerHTML = '<span class="ln cmd"><span class="p">$ </span>dctx mcp .   # ' + reqs.length + ' request(s) on stdin</span>' +
        res.responses.map(function (r) { return line('out', JSON.stringify(r, null, 1)); }).join('') +
        (res.stderr ? line('err', res.stderr) : '') + line('meta', '# exit ' + res.exit + ' · ' + res.ms + ' ms');
      title.textContent = 'dctx mcp'; say('Done.', 'good');
    }, fail);
  });
  fetch('/api/v1/health').then(function (r) { return r.ok ? r.json() : Promise.reject(); }).then(function (h) {
    say('Online: dctx ' + h.dctx + ' (' + h.sha + '), ' + h.samples + ' samples, ' + h.limits.requests_per_minute + ' runs per minute.', 'good');
  }, function () { offline.hidden = false; say('The playground service is offline. The recorded runs below were captured at build time.', 'bad'); });
})();
