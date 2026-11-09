/* dctx home showcase (pages/tour.py): five ways to use dctx, each step beside its real captured run.
   Every panel is complete HTML from the first paint. This script turns the list into tabs: a click, tap or
   arrow key shows that step's panel. With motion on, the steps advance by themselves: the progress bar of
   the current step is a CSS animation, and its end moves to the next step. Hover, focus, a hidden tab or
   the showcase being off screen pause it. With motion off or prefers-reduced-motion nothing moves and every
   step stays one click away; the output lines are always there, motion only lights them in order. */
(function () {
  'use strict';
  var root = document.documentElement, tour = document.querySelector('[data-tour]');
  if (!tour) return;
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  var tabs = [].slice.call(tour.querySelectorAll('[role="tab"]'));
  var panels = tabs.map(function (t) { return document.getElementById(t.getAttribute('aria-controls')); });
  var cur = 0, visible = true;
  function motion() { return root.classList.contains('motion') && !reduce.matches; }
  panels.forEach(function (p) {
    [].forEach.call(p.querySelectorAll('.ln'), function (l, k) { l.style.setProperty('--k', k); });
  });
  function paused() { tour.classList.toggle('paused', !visible || document.hidden); }
  function show(i, focus) {
    cur = (i + tabs.length) % tabs.length;
    tabs.forEach(function (t, k) {
      var on = k === cur;
      t.setAttribute('aria-selected', on ? 'true' : 'false');
      t.tabIndex = on ? 0 : -1;
      t.parentNode.classList.toggle('on', on);
      panels[k].classList.toggle('on', on);
      panels[k].classList.remove('lit');
    });
    if (focus) tabs[cur].focus();
    if (motion()) {
      // Restart the line reveal and the progress bar: drop the classes, force a style flush, add them back.
      var p = panels[cur], bar = tabs[cur].querySelector('.tour-bar i');
      if (bar) { bar.classList.remove('run'); void bar.offsetWidth; bar.classList.add('run'); }
      void p.offsetWidth; p.classList.add('lit');
    }
  }
  function mode() {
    var on = motion();
    tour.classList.toggle('auto', on);
    if (!on) tabs.forEach(function (t) { var b = t.querySelector('.tour-bar i'); if (b) b.classList.remove('run'); });
    show(cur);
  }
  tabs.forEach(function (t, k) {
    t.addEventListener('click', function () { show(k); });
    t.addEventListener('keydown', function (ev) {
      var d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[ev.key];
      if (d) { ev.preventDefault(); show(cur + d, true); }
      else if (ev.key === 'Home') { ev.preventDefault(); show(0, true); }
      else if (ev.key === 'End') { ev.preventDefault(); show(tabs.length - 1, true); }
    });
    var bar = t.querySelector('.tour-bar i');
    if (bar) bar.addEventListener('animationend', function () { if (motion() && k === cur) show(cur + 1); });
  });
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (en) { visible = en[0].isIntersecting; paused(); }, { threshold: 0.2 }).observe(tour);
  }
  document.addEventListener('visibilitychange', paused);
  document.addEventListener('dctx:motion', mode);
  tour.classList.add('ready');
  mode();
})();
