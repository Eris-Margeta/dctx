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
