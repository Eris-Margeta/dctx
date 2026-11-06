/* dctx website: theme toggle, menu dismissal, copy buttons. No network, no storage beyond the theme choice. */
(function () {
  var d = document, root = d.documentElement;
  root.classList.remove('js-off');

  var toggle = d.getElementById('theme-toggle');
  function current() {
    var t = root.getAttribute('data-theme');
    if (t) return t;
    return window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  function label() {
    if (toggle) toggle.setAttribute('aria-label', current() === 'dark' ? 'Switch to light theme' : 'Switch to dark theme');
  }
  if (toggle) {
    toggle.hidden = false;
    label();
    toggle.addEventListener('click', function () {
      var next = current() === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      try { localStorage.setItem('dctx-theme', next); } catch (e) {}
      label();
    });
  }

  var menu = d.querySelector('.menu');
  if (menu) {
    d.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && menu.open) { menu.open = false; menu.querySelector('summary').focus(); }
    });
    d.addEventListener('click', function (e) { if (menu.open && !menu.contains(e.target)) menu.open = false; });
  }

  d.querySelectorAll('.code').forEach(function (block) {
    var btn = block.querySelector('.copy'), pre = block.querySelector('pre');
    if (!btn || !pre || !navigator.clipboard) { if (btn) btn.remove(); return; }
    btn.hidden = false;
    btn.addEventListener('click', function () {
      var lines = pre.innerText.split('\n').filter(function (l) { return l.trim() && !/^\s*#/.test(l); })
        .map(function (l) { return l.replace(/^\$ /, '').replace(/\s+#.*$/, ''); });
      navigator.clipboard.writeText(lines.join('\n')).then(function () {
        btn.textContent = 'Copied';
        setTimeout(function () { btn.textContent = 'Copy'; }, 1600);
      }, function () { btn.textContent = 'Select'; });
    });
  });
})();
