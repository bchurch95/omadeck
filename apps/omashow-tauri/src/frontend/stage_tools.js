// Floating stage tools pill (edit mode): pen, freehand, nodes + snap.
// Strokes are session-local, stored per slide in EMU space so they stay
// anchored to the slide content regardless of window size.
const stageInk = {
  tool: null,        // "pen" | "freehand" | "nodes" | null
  snap: false,
  bySlide: new Map(), // slide index -> stroke objects
  activeStroke: null, // stroke in progress
  activeNodes: [],    // node tool: placed points {x, y} (slide fractions)
  dragging: null,     // node index being dragged
};

const STAGE_STROKE_STYLE = {
  pen: { color: "#0f172a", widthFrac: 0.0022, opacity: 1 },
  freehand: { color: "#ffd60a", widthFrac: 0.0085, opacity: 0.45 },
};
const GUIDE_POINTS = [1 / 3, 2 / 3];
const SNAP_EPS = 0.03;

function stageInkSvg() { return $("stage-ink"); }
function stageInkGroup() { return $("stage-ink-strokes"); }
function stageInkNodeGroup() {
  let g = $("stage-ink-nodes");
  if (!g && stageInkSvg()) {
    g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    g.id = "stage-ink-nodes";
    stageInkSvg().appendChild(g);
  }
  return g;
}
function stageInkDims() {
  const d = slideDimensions();
  return { w: d.width_emu, h: d.height_emu };
}
function snapAxis(v) {
  for (const p of [0.5, ...GUIDE_POINTS]) {
    if (Math.abs(v - p) < SNAP_EPS) return p;
  }
  return v;
}
function stageInkNorm(e) {
  const r = stageInkSvg().getBoundingClientRect();
  let x = (e.clientX - r.left) / r.width;
  let y = (e.clientY - r.top) / r.height;
  if (stageInk.snap) { x = snapAxis(x); y = snapAxis(y); }
  return { x: Math.min(1, Math.max(0, x)), y: Math.min(1, Math.max(0, y)) };
}
function stageInkEmuPoints(pts) {
  const { w, h } = stageInkDims();
  const out = [];
  for (let i = 0; i + 1 < pts.length; i += 2) {
    out.push((pts[i] * w).toFixed(1) + "," + (pts[i + 1] * h).toFixed(1));
  }
  return out.join(" ");
}
function stageInkStrokeEl(s) {
  const { w } = stageInkDims();
  const st = STAGE_STROKE_STYLE[s.tool];
  const el = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
  el.setAttribute("points", stageInkEmuPoints(s.points));
  el.setAttribute("fill", "none");
  el.setAttribute("stroke", st.color);
  el.setAttribute("stroke-width", (st.widthFrac * w).toFixed(1));
  el.setAttribute("stroke-linecap", "round");
  el.setAttribute("stroke-linejoin", "round");
  el.setAttribute("stroke-opacity", st.opacity);
  return el;
}
function stageInkNodeEl(p, i) {
  const { w, h } = stageInkDims();
  const css = stageInkSvg().getBoundingClientRect().width;
  const r = 12 * (w / css);
  const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
  c.setAttribute("cx", (p.x * w).toFixed(1));
  c.setAttribute("cy", (p.y * h).toFixed(1));
  c.setAttribute("r", r.toFixed(1));
  c.setAttribute("fill", "#4f46e5");
  c.setAttribute("stroke", "#ffffff");
  c.setAttribute("stroke-width", (r * 0.25).toFixed(1));
  c.dataset.nodeIdx = i;
  return c;
}
function syncStageInkButtons() {
  document.querySelectorAll("#pen-mode-seg button").forEach((b) => {
    b.classList.toggle("on", b.dataset.mode === stageInk.tool);
  });
  const snap = $("snap-toggle");
  if (snap) snap.classList.toggle("on", stageInk.snap);
  const undo = $("stage-ink-undo");
  const clear = $("stage-ink-clear");
  const empty = stageInk.activeNodes.length === 0 && !(stageInk.bySlide.get(currentSlide) || []).length;
  if (undo) undo.disabled = empty;
  if (clear) clear.disabled = empty;
}
function renderStageInk() {
  if (!stageInkSvg() || typeof currentSlide === "undefined" || currentSlide < 0) return;
  const { w, h } = stageInkDims();
  stageInkSvg().setAttribute("viewBox", "0 0 " + w + " " + h);
  const g = stageInkGroup();
  g.innerHTML = "";
  for (const s of stageInk.bySlide.get(currentSlide) || []) g.appendChild(stageInkStrokeEl(s));
  if (stageInk.activeStroke) g.appendChild(stageInkStrokeEl(stageInk.activeStroke));
  const ng = stageInkNodeGroup();
  ng.innerHTML = "";
  for (let i = 0; i < stageInk.activeNodes.length; i++) ng.appendChild(stageInkNodeEl(stageInk.activeNodes[i], i));
  if (stageInk.activeNodes.length >= 2) {
    const line = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
    line.setAttribute("points", stageInkEmuPoints(stageInk.activeNodes.flatMap((p) => [p.x, p.y])));
    line.setAttribute("fill", "none");
    line.setAttribute("stroke", "#4f46e5");
    line.setAttribute("stroke-width", (0.0022 * w).toFixed(1));
    line.setAttribute("stroke-dasharray", "14 10");
    ng.appendChild(line);
  }
  syncStageInkButtons();
}
function stageInkStrokeFrom(tool, e) {
  const p0 = stageInkNorm(e);
  return { tool, points: [p0.x, p0.y, p0.x, p0.y] };
}
function stageInkExtend(stroke, e) {
  const p = stageInkNorm(e);
  const pts = stroke.points;
  const dx = p.x - pts[pts.length - 2];
  const dy = p.y - pts[pts.length - 1];
  const minDist = stroke.tool === "freehand" ? 0.0008 : 0.004;
  if (dx * dx + dy * dy >= minDist * minDist) pts.push(p.x, p.y);
}
function stageInkFinishStroke(stroke) {
  if (!model || currentSlide < 0) return;
  const pts = stroke.points;
  const p0x = pts[0], p0y = pts[1];
  const p1x = pts[pts.length - 2], p1y = pts[pts.length - 1];
  if (stroke.tool === "freehand" && pts.length < 4) return; // ignore taps
  if (stroke.tool === "pen" && p0x === p1x && p0y === p1y) return; // dot -> ignore
  const arr = stageInk.bySlide.get(currentSlide) || [];
  arr.push(stroke);
  stageInk.bySlide.set(currentSlide, arr);
}
function hitNode(e) {
  const { w, h } = stageInkDims();
  const r = stageInkSvg().getBoundingClientRect();
  const nx = (e.clientX - r.left) / r.width;
  const ny = (e.clientY - r.top) / r.height;
  const ex = 14 / r.width;  // ~14 css px, in slide fractions
  const ey = 14 / r.height;
  for (let i = 0; i < stageInk.activeNodes.length; i++) {
    const p = stageInk.activeNodes[i];
    if (Math.abs(p.x - nx) < ex && Math.abs(p.y - ny) < ey) return i;
  }
  return null;
}
function finishNodeChain() {
  if (stageInk.activeNodes.length >= 2 && model && currentSlide >= 0) {
    const pts = stageInk.activeNodes.flatMap((p) => [p.x, p.y]);
    const clean = [pts[0], pts[1]];
    for (let i = 2; i < pts.length; i += 2) {
      if (pts[i] !== clean[clean.length - 2] || pts[i + 1] !== clean[clean.length - 1]) clean.push(pts[i], pts[i + 1]);
    }
    if (clean.length >= 4) {
      const arr = stageInk.bySlide.get(currentSlide) || [];
      arr.push({ tool: "nodes", points: clean });
      stageInk.bySlide.set(currentSlide, arr);
    }
  }
  stageInk.activeStroke = null;
  stageInk.activeNodes = [];
  stageInk.dragging = null;
  renderStageInk();
}
function syncStageInkOverlay() {
  const svg = stageInkSvg();
  if (!svg) return;
  svg.style.pointerEvents = stageInk.tool && !presenting() ? "auto" : "none";
  svg.style.cursor = stageInk.tool ? "crosshair" : "default";
}
function setStageInkTool(mode) {
  if (presenting() || !model) return;
  stageInk.activeStroke = null;
  stageInk.activeNodes = [];
  stageInk.dragging = null;
  stageInk.tool = stageInk.tool === mode ? null : mode;
  closeTextEdit(false);
  syncStageInkOverlay();
  renderStageInk();
}
function stageInkUndo() {
  if (stageInk.activeNodes.length) {
    if (stageInk.dragging != null) { stageInk.dragging = null; renderStageInk(); return; }
    stageInk.activeNodes.pop();
    renderStageInk();
    return;
  }
  const arr = stageInk.bySlide.get(currentSlide);
  if (!arr || !arr.length) return;
  arr.pop();
  if (!arr.length) stageInk.bySlide.delete(currentSlide);
  renderStageInk();
  flash("stage mark removed");
}
function stageInkClear() {
  stageInk.activeStroke = null;
  stageInk.activeNodes = [];
  stageInk.dragging = null;
  stageInk.bySlide.delete(currentSlide);
  renderStageInk();
  flash("slide marks cleared");
}
// hooks called from main.js
function stageInkOnSlideChanged() { renderStageInk(); }
function stageInkOnModelLoaded() {
  stageInk.bySlide = new Map();
  stageInk.activeStroke = null;
  stageInk.activeNodes = [];
  renderStageInk();
}
function stageInkPointerDown(e) {
  if (!stageInk.tool || presenting() || !model) return;
  const svg = stageInkSvg();
  svg.setPointerCapture(e.pointerId);
  if (stageInk.tool === "nodes") {
    const hit = hitNode(e);
    if (hit != null) { stageInk.dragging = hit; return; }
    stageInk.activeNodes.push(stageInkNorm(e));
    renderStageInk();
    return;
  }
  stageInk.activeStroke = stageInkStrokeFrom(stageInk.tool, e);
  renderStageInk();
}
function stageInkPointerMove(e) {
  if (stageInk.tool === "nodes") {
    if (stageInk.dragging != null) {
      stageInk.activeNodes[stageInk.dragging] = stageInkNorm(e);
      renderStageInk();
    }
    return;
  }
  if (!stageInk.activeStroke) return;
  stageInkExtend(stageInk.activeStroke, e);
  renderStageInk();
}
function stageInkPointerUp() {
  if (stageInk.tool === "nodes") {
    if (stageInk.dragging != null) { stageInk.dragging = null; renderStageInk(); }
    return;
  }
  if (!stageInk.activeStroke) return;
  stageInkFinishStroke(stageInk.activeStroke);
  stageInk.activeStroke = null;
  renderStageInk();
}
document.querySelectorAll("#pen-mode-seg button").forEach((b) => {
  b.addEventListener("click", () => setStageInkTool(b.dataset.mode));
});
const _snapBtn = $("snap-toggle");
if (_snapBtn) _snapBtn.addEventListener("click", () => {
  stageInk.snap = !stageInk.snap;
  renderStageInk();
});
const _undoBtn = $("stage-ink-undo");
if (_undoBtn) _undoBtn.addEventListener("click", stageInkUndo);
const _clearBtn = $("stage-ink-clear");
if (_clearBtn) _clearBtn.addEventListener("click", stageInkClear);
(function stageInkWireCanvas() {
  const svg = $("stage-ink");
  if (!svg) return;
  svg.addEventListener("pointerdown", stageInkPointerDown);
  svg.addEventListener("pointermove", stageInkPointerMove);
  svg.addEventListener("pointerup", stageInkPointerUp);
  svg.addEventListener("pointercancel", stageInkPointerUp);
  svg.addEventListener("dblclick", (e) => {
    if (stageInk.tool === "nodes") { e.preventDefault(); finishNodeChain(); }
  });
  syncStageInkOverlay();
  // keep overlay interactive state in step with app mode changes
  new MutationObserver(syncStageInkOverlay).observe(document.body, {
    attributes: true, attributeFilter: ["class", "data-mode"],
  });
})();
// keyboard: finish/cancel node chain
window.addEventListener("keydown", (e) => {
  if (stageInk.tool !== "nodes" || !stageInk.activeNodes.length || presenting()) return;
  if (e.key === "Enter") { e.preventDefault(); finishNodeChain(); }
  else if (e.key === "Escape") { e.preventDefault(); stageInk.activeNodes = []; stageInk.dragging = null; renderStageInk(); }
});

// structural ops: keep per-slide marks attached to the right slides
function stageInkInsertSlide(idx) {
  const next = new Map();
  for (const [k, v] of stageInk.bySlide) next.set(k >= idx ? k + 1 : k, v);
  stageInk.bySlide = next;
  renderStageInk();
}
function stageInkDeleteSlide(idx) {
  const next = new Map();
  for (const [k, v] of stageInk.bySlide) {
    if (k === idx) continue;
    next.set(k > idx ? k - 1 : k, v);
  }
  stageInk.bySlide = next;
  renderStageInk();
}
function stageInkMoveSlide(from, to) {
  if (from === to) return;
  const next = new Map();
  for (const [k, v] of stageInk.bySlide) {
    let nk = k;
    if (k === from) nk = to;
    else if (from < to && k > from && k <= to) nk = k - 1;
    else if (from > to && k >= to && k < from) nk = k + 1;
    next.set(nk, v);
  }
  stageInk.bySlide = next;
  renderStageInk();
}
function stageInkReorder(order) {
  const arr = Array.from(stageInk.bySlide.entries());
  const next = new Map();
  order.forEach((oldIdx, newIdx) => {
    const e = arr.find((p) => p[0] === oldIdx);
    if (e) next.set(newIdx, e[1]);
  });
  stageInk.bySlide = next;
  renderStageInk();
}

function initStageTools() {
  syncStageInkOverlay();
  renderStageInk();
}
