/* dctx hero: one real run on the sample project, folder -> policy -> graph -> bounded page. The final
   state is plain HTML and SVG built from the captured data (pages/flow.py); with motion on, this plays
   the four steps in order with CSS transitions when the figure scrolls into view. With motion off or
   prefers-reduced-motion, nothing moves and the final state stays. It also starts the hero terminal. */
(function () {
  'use strict';
  var root = document.documentElement, flow = document.querySelector('[data-flow]');
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)'), timers = [], seen = false, started = false;
  function motion() { return root.classList.contains('motion') && !reduce.matches; }
  function scene() { document.dispatchEvent(new CustomEvent('dctx:scene')); }
  function boot(f) { (window.requestIdleCallback || function (g) { setTimeout(g, 120); })(f, { timeout: 900 }); }
  if (!flow) { if (document.readyState === 'complete') boot(scene); else window.addEventListener('load', function () { boot(scene); }); return; }
  var steps = [].slice.call(flow.querySelectorAll('[data-step]')), btn = flow.querySelector('[data-replay]');
  ['.files li', '.ranked li', '.page .pl'].forEach(function (sel) {
    [].forEach.call(flow.querySelectorAll(sel), function (el, i) { el.style.setProperty('--k', i); });
  });
  // Stage n is reached at AT[n] ms; the step it lights is ON[n] (1 and 2 share the repository panel).
  var AT = [0, 250, 1700, 3100, 4700, 7200], ON = [0, 1, 1, 3, 4, 0];
  function clear() { timers.forEach(clearTimeout); timers = []; }
  function set(n) {
    for (var i = 1; i <= 5; i++) flow.classList.toggle('s' + i, i <= n);
    steps.forEach(function (s) { s.classList.toggle('on', +s.getAttribute('data-step') === ON[n]); });
  }
  function finish() { clear(); flow.classList.remove('play'); set(5); if (btn) btn.hidden = !motion(); }
  function play() {
    clear();
    if (!motion()) { finish(); return; }
    started = true; flow.classList.add('play'); set(0); if (btn) btn.hidden = false;
    // Two frames so the hidden state is painted before the first transition starts.
    requestAnimationFrame(function () { requestAnimationFrame(function () {
      AT.forEach(function (t, n) { if (n) timers.push(setTimeout(function () { set(n); if (n === 5) flow.classList.remove('play'); }, t)); });
    }); });
  }
  function arm() {
    if (!motion()) { finish(); return; }
    flow.classList.add('play'); set(0);
    if (seen) play();
  }
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (en) {
      if (en[0].isIntersecting && !seen) { seen = true; if (motion() && !started) play(); }
    }, { threshold: 0.25 }).observe(flow);
  } else seen = true;
  if (btn) btn.addEventListener('click', play);
  document.addEventListener('dctx:motion', function () { started = false; if (motion()) arm(); else finish(); });
  if (motion()) arm(); else finish();
  // The terminal starts after the load event, when the main thread is idle.
  if (document.readyState === 'complete') boot(scene); else window.addEventListener('load', function () { boot(scene); });
})();
