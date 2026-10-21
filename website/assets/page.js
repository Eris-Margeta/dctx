/* dctx.app article pages: header state, mobile menu, motion preference. */
(function () {
  'use strict';
  var header = document.querySelector('.site-header'), nav = document.querySelector('.nav'), toggle = document.querySelector('.menu-toggle');
  function onScroll() { header.classList.toggle('scrolled', window.scrollY > 8); }
  onScroll(); window.addEventListener('scroll', onScroll, { passive: true });
  if (toggle) {
    toggle.addEventListener('click', function () {
      var open = nav.classList.toggle('open');
      toggle.setAttribute('aria-expanded', String(open)); toggle.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
    });
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && nav.classList.contains('open')) { nav.classList.remove('open'); toggle.setAttribute('aria-expanded', 'false'); toggle.focus(); } });
  }
  var mt = document.getElementById('motion-toggle');
  if (mt) {
    var off = false; try { off = localStorage.getItem('dctx-motion') === 'off'; } catch (e) {}
    mt.setAttribute('aria-pressed', String(off));
    mt.addEventListener('click', function () {
      off = !off; mt.setAttribute('aria-pressed', String(off));
      try { localStorage.setItem('dctx-motion', off ? 'off' : 'on'); } catch (e) {}
      document.documentElement.classList.toggle('motion', !off);
    });
  }
})();
