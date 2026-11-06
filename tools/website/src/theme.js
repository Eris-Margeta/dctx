/* Runs before first paint: theme and motion preference, so there is no flash. */
(function () {
  var d = document.documentElement, t = null, m = null;
  d.className = d.className.replace('no-js', 'js');
  try { t = localStorage.getItem('dctx-theme'); m = localStorage.getItem('dctx-motion'); } catch (e) {}
  if (t === 'light' || t === 'dark') d.setAttribute('data-theme', t);
  if (m !== 'off' && window.matchMedia && window.matchMedia('(prefers-reduced-motion: no-preference)').matches) d.classList.add('motion');
})();
