/* Offline social renderer. Only renderFrame(seconds) changes the scene: no RAF or clock. */
(function () {
  'use strict';
  var canvas = document.querySelector('canvas'), ctx = canvas.getContext('2d');
  var base = new Image(), scene;
  window.loadCard = async function (url, geometry) {
    scene = geometry;
    base.src = url;
    await base.decode();
  };
  window.renderFrame = function (seconds) {
    ctx.drawImage(base, 0, 0, 1200, 630);
    // Reuse the captured hero graph, with a gentle signal moving along its real edges.
    // Sin/cos make both position and brightness continuous across the three-second seam.
    var phase = seconds / 3 * Math.PI * 2;
    scene.edges.slice(0, 8).forEach(function (edge, i) {
      var a = scene.nodes[edge[0]], b = scene.nodes[edge[1]];
      var p = (1 - Math.cos(phase + i * .7)) / 2;
      var x = 560 + (a.x + (b.x - a.x) * p) * 620 / scene.w;
      var y = 70 + (a.y + (b.y - a.y) * p) * 326 / scene.h;
      ctx.fillStyle = 'rgba(34,211,238,.65)';
      ctx.beginPath(); ctx.arc(x, y, 2.1, 0, Math.PI * 2); ctx.fill();
    });
  };
}());
