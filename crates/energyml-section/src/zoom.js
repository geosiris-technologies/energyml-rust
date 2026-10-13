/* Zoom for a section-view SVG opened on its own (a browser, not the page).
 *
 * Written by energyml-section and read by nothing else. Plain ES2015, no
 * dependency, no network. Where scripts do not run the drawing is complete and
 * static; this only adds a wheel, a drag and a double click.
 *
 *   wheel          zoom about the pointer, both axes
 *   Shift + wheel  horizontal axis only        Alt + wheel  vertical axis only
 *   drag           pan                         double click  back to the whole
 *   Ctrl + wheel   left to the browser's own zoom
 *
 * What it reads, all written by svg.rs: on the root `data-frame` ("x y w h",
 * pixels), `data-window` ("x0 x1 y0 y1", the drawing's units) and `data-scale`
 * (pixels per unit); on `g[data-zoom=plot]` the plot, clipped by a parent; on
 * `[data-zoom-fixed="x y"]` a dot or text that keeps its pixel size around its
 * anchor; on `[data-zoom-anchor="x y"]` a label of the unclipped labels layer;
 * on `g[data-ticks=x|y]` the graduations, which it writes again for the window
 * shown (the same rule as layout/ticks.rs: 1, 2 or 5 times a power of ten,
 * about frame.w/90 ticks across and frame.h/40 up).
 *
 * The state is the map from the drawing's pixels to the screen's:
 * p' = k * p + t, per axis. The plot group carries it as a matrix; strokes are
 * `non-scaling-stroke`, so they keep their width on their own.
 */
(function () {
  "use strict";
  var NS = "http://www.w3.org/2000/svg";
  var MAX_K = 2000;
  var script = document.currentScript;
  var svg = (script && script.ownerSVGElement) || document.documentElement;
  if (!svg || svg.localName !== "svg" || !svg.createSVGPoint) return;

  function numbers(name, n) {
    var parts = (svg.getAttribute(name) || "").trim().split(/\s+/).map(Number);
    return parts.length === n && parts.every(isFinite) ? parts : null;
  }
  var frame = numbers("data-frame", 4);
  var win = numbers("data-window", 4);
  var scale = numbers("data-scale", 2);
  if (!frame || !win || !scale || !(scale[0] > 0) || !(scale[1] > 0)) return;
  var fx = frame[0], fy = frame[1], fw = frame[2], fh = frame[3];

  var plots = [].slice.call(svg.querySelectorAll("g[data-zoom=plot]"));
  var fixed = [].slice.call(svg.querySelectorAll("[data-zoom-fixed]")).map(function (el) {
    return { el: el, at: numbers2(el.getAttribute("data-zoom-fixed")) };
  });
  var labels = [].slice.call(svg.querySelectorAll("[data-zoom-anchor]")).map(function (el) {
    return { el: el, at: numbers2(el.getAttribute("data-zoom-anchor")), leaders: [].slice.call(el.querySelectorAll("polyline")) };
  });
  var axisX = svg.querySelector("g[data-ticks=x]");
  var axisY = svg.querySelector("g[data-ticks=y]");
  var movedX = axisX && axisX.getAttribute("data-ticks-mode") === "move"
    ? [].slice.call(axisX.querySelectorAll("[data-tick-at]")).map(function (el) { return { el: el, at: Number(el.getAttribute("data-tick-at")) }; })
    : null;

  function numbers2(text) {
    var p = (text || "").trim().split(/\s+/).map(Number);
    return p.length === 2 && isFinite(p[0]) && isFinite(p[1]) ? p : [0, 0];
  }

  var kx = 1, ky = 1, tx = 0, ty = 0;
  var pending = false;

  /* A client point as the drawing's own coordinates (the viewBox may be scaled). */
  function toUser(clientX, clientY) {
    var m = svg.getScreenCTM();
    if (!m) return null;
    var p = svg.createSVGPoint();
    p.x = clientX; p.y = clientY;
    p = p.matrixTransform(m.inverse());
    return [p.x, p.y];
  }
  function inFrame(p) {
    return !!p && p[0] >= fx && p[0] <= fx + fw && p[1] >= fy && p[1] <= fy + fh;
  }
  /* The whole original frame must stay covering the frame: no empty margin. */
  function clamp() {
    kx = Math.min(MAX_K, Math.max(1, kx));
    ky = Math.min(MAX_K, Math.max(1, ky));
    tx = Math.min(fx - kx * fx, Math.max(fx + fw - kx * (fx + fw), tx));
    ty = Math.min(fy - ky * fy, Math.max(fy + fh - ky * (fy + fh), ty));
  }
  function schedule() {
    if (pending) return;
    pending = true;
    (window.requestAnimationFrame || function (f) { return setTimeout(f, 16); })(function () {
      pending = false;
      apply();
    });
  }

  /* ---- graduations: the port of layout/ticks.rs ---- */
  function niceStep(range, target) {
    if (!(isFinite(range) && range > 0 && target >= 1)) return 1;
    var raw = range / target;
    var magnitude = Math.pow(10, Math.floor(Math.log10(raw)));
    var norm = raw / magnitude;
    return (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10) * magnitude;
  }
  function tickLabel(value, step) {
    var decimals = step >= 1 ? 0 : Math.min(20, -Math.floor(Math.log10(step)));
    var text = value.toFixed(decimals);
    return /^-0*\.?0*$/.test(text) ? text.slice(1) : text;
  }
  function make(name, attributes, text) {
    var el = document.createElementNS(NS, name);
    for (var key in attributes) el.setAttribute(key, attributes[key]);
    if (text !== undefined) el.textContent = text;
    return el;
  }
  function num(v) {
    var t = (Math.round(v * 100) / 100).toString();
    return t === "-0" ? "0" : t;
  }
  /* Values in [lo, hi] at multiples of the step, as the library computes them. */
  function graduate(lo, hi, target) {
    var step = niceStep(hi - lo, target);
    var out = [];
    if (!(hi > lo)) return { step: step, values: out };
    var first = Math.ceil(lo / step - 1e-9), last = Math.floor(hi / step + 1e-9);
    for (var k = first; k <= last && out.length < 400; k++) out.push(k * step);
    return { step: step, values: out };
  }

  function regradeX() {
    if (!axisX || movedX) return;
    var lo = win[0] + ((fx - tx) / kx - fx) / scale[0];
    var hi = win[0] + ((fx + fw - tx) / kx - fx) / scale[0];
    var g = graduate(lo, hi, fw / 90);
    var frag = document.createDocumentFragment();
    g.values.forEach(function (v) {
      var at = kx * (fx + (v - win[0]) * scale[0]) + tx;
      if (at < fx - 0.5 || at > fx + fw + 0.5) return;
      frag.appendChild(make("line", { x1: num(at), y1: num(fy), x2: num(at), y2: num(fy + fh), stroke: "#e0e0e0", "stroke-width": "1" }));
      frag.appendChild(make("text", { x: num(at), y: num(fy + fh + 14), "text-anchor": "middle" }, tickLabel(v, g.step)));
    });
    while (axisX.firstChild) axisX.removeChild(axisX.firstChild);
    axisX.appendChild(frag);
  }
  function regradeY() {
    if (!axisY) return;
    /* y' = ky * y + ty, and a value v sits at y = fy + fh - (v - win.y0) * scale.y */
    var lo = win[2] + (fy + fh - (fy + fh - ty) / ky) / scale[1];
    var hi = win[2] + (fy + fh - (fy - ty) / ky) / scale[1];
    var g = graduate(lo, hi, fh / 40);
    var frag = document.createDocumentFragment();
    g.values.forEach(function (v) {
      var at = ky * (fy + fh - (v - win[2]) * scale[1]) + ty;
      if (at < fy - 0.5 || at > fy + fh + 0.5) return;
      frag.appendChild(make("line", { x1: num(fx), y1: num(at), x2: num(fx + fw), y2: num(at), stroke: "#e0e0e0", "stroke-width": "1" }));
      frag.appendChild(make("text", { x: num(fx - 5), y: num(at + 4), "text-anchor": "end" }, tickLabel(v, g.step)));
    });
    while (axisY.firstChild) axisY.removeChild(axisY.firstChild);
    axisY.appendChild(frag);
  }

  function apply() {
    var identity = kx === 1 && ky === 1 && tx === 0 && ty === 0;
    var matrix = identity ? null : "matrix(" + kx + " 0 0 " + ky + " " + tx + " " + ty + ")";
    plots.forEach(function (g) {
      if (matrix) g.setAttribute("transform", matrix); else g.removeAttribute("transform");
    });
    /* Dots and texts inside the plot: the group scales them, they scale back. */
    fixed.forEach(function (f) {
      if (identity) { f.el.removeAttribute("transform"); return; }
      f.el.setAttribute("transform", "translate(" + f.at[0] + " " + f.at[1] + ") scale(" + 1 / kx + " " + 1 / ky + ") translate(" + -f.at[0] + " " + -f.at[1] + ")");
    });
    /* Labels of the unclipped layer: moved to where their anchor went, at their
       own size; one reaching above the frame follows the horizontal axis only,
       and one that left the frame is hidden. Their dotted leaders no longer
       reach their head, so they go while the drawing is magnified. */
    labels.forEach(function (l) {
      var ax = kx * l.at[0] + tx, ay = ky * l.at[1] + ty;
      var inside = l.at[1] >= fy && l.at[1] <= fy + fh;
      var visible = ax >= fx && ax <= fx + fw && (!inside || (ay >= fy && ay <= fy + fh));
      l.el.style.display = visible ? "" : "none";
      if (identity) l.el.removeAttribute("transform");
      else l.el.setAttribute("transform", "translate(" + (ax - l.at[0]) + " " + (inside ? ay - l.at[1] : 0) + ")");
      l.leaders.forEach(function (p) { p.style.display = identity ? "" : "none"; });
    });
    if (movedX) {
      movedX.forEach(function (m) {
        var at = kx * m.at + tx;
        m.el.style.display = at >= fx - 0.5 && at <= fx + fw + 0.5 ? "" : "none";
        m.el.setAttribute("transform", "translate(" + (at - m.at) + " 0)");
      });
    } else regradeX();
    regradeY();
  }

  /* ---- input ---- */
  function zoomBy(factor, axes, p) {
    var nkx = kx, nky = ky;
    if (axes !== "y") nkx = Math.min(MAX_K, Math.max(1, kx * factor));
    if (axes !== "x") nky = Math.min(MAX_K, Math.max(1, ky * factor));
    /* the point under the pointer stays under it */
    tx = p[0] - (p[0] - tx) * (nkx / kx);
    ty = p[1] - (p[1] - ty) * (nky / ky);
    kx = nkx; ky = nky;
    clamp();
    schedule();
  }
  svg.addEventListener("wheel", function (e) {
    if (e.ctrlKey || e.metaKey) return; /* the browser's own zoom */
    var p = toUser(e.clientX, e.clientY);
    if (!inFrame(p)) return;
    var delta = e.deltaY || e.deltaX;
    if (!delta) return;
    var unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    e.preventDefault();
    zoomBy(Math.exp(-delta * unit * 0.002), e.shiftKey ? "x" : e.altKey ? "y" : "xy", p);
  }, { passive: false });

  var drag = null;
  svg.addEventListener("pointerdown", function (e) {
    if (e.button !== 0 || !inFrame(toUser(e.clientX, e.clientY))) return;
    drag = { x: e.clientX, y: e.clientY, id: e.pointerId };
    try { svg.setPointerCapture(e.pointerId); } catch (_) { /* a synthetic pointer */ }
    svg.style.cursor = "grabbing";
  });
  svg.addEventListener("pointermove", function (e) {
    if (!drag || e.pointerId !== drag.id) return;
    var a = toUser(drag.x, drag.y), b = toUser(e.clientX, e.clientY);
    if (!a || !b) return;
    tx += b[0] - a[0];
    ty += b[1] - a[1];
    drag.x = e.clientX; drag.y = e.clientY;
    clamp();
    schedule();
  });
  function drop(e) {
    if (!drag || e.pointerId !== drag.id) return;
    drag = null;
    svg.style.cursor = "";
  }
  svg.addEventListener("pointerup", drop);
  svg.addEventListener("pointercancel", drop);
  svg.addEventListener("dblclick", function (e) {
    if (!inFrame(toUser(e.clientX, e.clientY))) return;
    kx = ky = 1; tx = ty = 0;
    schedule();
  });
})();
