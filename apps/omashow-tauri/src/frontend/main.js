// Omashow frontend — talks to the Rust core via Tauri commands.
// The Rust side owns the full Presentation; we only ever see its
// lightweight JSON projection and send back small edits.

const $ = (id) => document.getElementById(id);
const invoke = (cmd, args = {}) => window.__TAURI__.core.invoke(cmd, args);

let model = null;          // current PresentationModel projection
let currentSlide = -1;     // selected slide index
let filePath = null;

// ---------- helpers ----------
function debounce(fn, ms) {
  let t = null;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...args), ms);
  };
}

function flash(msg, cls = "ok") {
  const el = $("status-msg");
  el.textContent = msg;
  el.className = cls;
  clearTimeout(flash._t);
  flash._t = setTimeout(() => { el.textContent = ""; }, 4000);
}

function setPath(p) {
  filePath = p;
  $("status-path").textContent = p ? p : "no file (unsaved)";
}

function markState(s) {
  const el = $("doc-state");
  el.textContent = s;
  el.classList.toggle("edited", s === "edited");
}

function updateNotesCount() {
  const v = $("notes-input").value.trim();
  $("notes-count").textContent = v ? v.split(/\s+/).length + " words" : "";
}

// ---------- rendering ----------
function hasDeck() {
  $("empty-msg").style.display = model ? "none" : "flex";
  $("notes-bar").style.display = model && model.slides.length > 0 ? "block" : "none";
}

// ---------- slide content cache ----------
// Per-slide shape content, fetched lazily from get_slide_content and cached
// here so both the filmstrip and the canvas can paint from it.
const slideContents = new Map();

function slideDimensions() {
  if (currentSlide >= 0) {
    const cached = slideContents.get(currentSlide);
    if (cached) return cached.slide_dimensions;
  }
  if (model && model.slide_dimensions) return model.slide_dimensions;
  return { width_emu: 12_192_000, height_emu: 6_858_000 };
}

let zoomMode = "fit"; // "fit" | percent string ("25".."200"), 100% = 96 CSS px/in
let lastFitPct = null; // effective percentage of the last "fit" layout

// Fit #slide-canvas into #preview-wrap (or the whole window while presenting)
// at the slide's true aspect ratio, or render at an explicit zoom percentage.
function fitCanvas() {
  const d = slideDimensions();
  const canvas = $("slide-canvas");
  if (zoomMode !== "fit") {
    const pct = (parseFloat(zoomMode) || 100) / 100;
    canvas.style.width = (d.width_emu / 914400) * 96 * pct + "px";
    canvas.style.height = (d.height_emu / 914400) * 96 * pct + "px";
    return;
  }
  // While presenting, #preview-wrap already shrinks to make room for the
  // console side panel, so its box is the right reference in both modes.
  const wrapW = $("preview-wrap").clientWidth;
  const wrapH = $("preview-wrap").clientHeight;
  if (wrapW < 50 || wrapH < 50) return; // sorter mode: stage hidden, keep last fit
  const availW = Math.max(100, wrapW - 56);
  const availH = Math.max(100, wrapH - 56);
  let w = availW;
  let h = (w * d.height_emu) / d.width_emu;
  if (h > availH) { h = availH; w = (h * d.width_emu) / d.height_emu; }
  canvas.style.width = w + "px";
  canvas.style.height = h + "px";
  lastFitPct = Math.round((w / ((d.width_emu / 914400) * 96)) * 100);
  syncZoomUI();
}

function setZoom(mode) {
  zoomMode = mode;
  if (currentSlide >= 0) paintCurrentSlide();
}

// ---------- present mode: fullscreen slideshow on this screen ----------
function presenting() { return document.body.classList.contains("presenting"); }

let presentHideTimer = null;
function enterPresent() {
  if (!model || currentSlide < 0) { flash("open a deck first", "err"); return; }
  document.body.classList.add("presenting");
  // Fullscreen the whole document (not just #main) so the presenter
  // topbar, a sibling of the content area, stays visible.
  if (document.documentElement.requestFullscreen) {
    document.documentElement.requestFullscreen().catch(() => {});
  }
  setShutter("clear");
  paintCurrentSlide();
  updateConsole();
  updateBuildStepper();
  startTimer();
}
function exitPresent() {
  document.body.classList.remove("presenting");
  document.body.classList.remove("chrome-visible");
  stopTimer();
  shutter = "clear";
  ["black", "white", "freeze"].forEach((m) => $("btn-shutter-" + m).classList.remove("on"));
  laserTool = false;
  laserCtrl = false;
  $("btn-laser").classList.remove("on");
  resetInk();
  emitToAudience("laser-move", { on: false });
  invoke("close_audience_window").catch(() => {});
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  resetModeToEdit();
}

// ---------- slide grid navigator: G while presenting ----------
function gridOpen() { return document.body.classList.contains("grid-open"); }
function closeSlideGrid() { document.body.classList.remove("grid-open"); }
function openSlideGrid() {
  if (!model || !presenting()) return;
  const cells = $("grid-cells");
  cells.innerHTML = "";
  model.slides.forEach((s, i) => {
    const cell = document.createElement("div");
    cell.className = "grid-cell" + (i === currentSlide ? " current" : "");

    const thumb = document.createElement("div");
    thumb.className = "grid-thumb";
    const num = document.createElement("span");
    num.className = "grid-num";
    num.textContent = i + 1;
    const label = document.createElement("div");
    label.className = "grid-label";
    label.textContent = s.title || "";
    label.title = s.title || "";
    cell.append(thumb, num, label);

    cell.addEventListener("click", () => {
      closeSlideGrid();
      selectSlide(i);
    });
    cells.appendChild(cell);
  });
  document.body.classList.add("grid-open");
  model.slides.forEach((_, i) => {
    const thumb = document.querySelectorAll("#grid-cells .grid-thumb")[i];
    const cached = slideContents.get(i);
    if (cached && thumb) renderSlideInto(thumb, cached, true);
    else fetchSlideContent(i);
  });
}
function toggleSlideGrid() {
  if (!presenting()) return;
  if (gridOpen()) closeSlideGrid();
  else openSlideGrid();
}

// ---------- presenter console: timers, target countdown ----------
let presentStart = 0;
let timerInt = null;
let timerPaused = false;
let pausedElapsedMs = 0;
let targetMinutes = 20;
function fmtHMS(ms) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), ss = s % 60;
  return String(h).padStart(2, "0") + ":" + String(m).padStart(2, "0") + ":" + String(ss).padStart(2, "0");
}
function elapsedMs() {
  return timerPaused ? pausedElapsedMs : Date.now() - presentStart;
}
function tickTimer() {
  $("timer-elapsed").textContent = fmtHMS(elapsedMs());
  const targetMs = targetMinutes * 60000;
  const remaining = Math.max(0, targetMs - elapsedMs());
  const cd = $("timer-countdown");
  cd.textContent = fmtHMS(remaining);
  cd.classList.toggle("low", remaining < targetMs * 0.1);
  $("timer-clock").textContent = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
function startTimer() {
  presentStart = Date.now();
  timerPaused = false;
  pausedElapsedMs = 0;
  $("btn-timer-pause").textContent = "Pause";
  tickTimer();
  clearInterval(timerInt);
  timerInt = setInterval(tickTimer, 1000);
}
function stopTimer() {
  clearInterval(timerInt);
  timerInt = null;
}
function toggleTimerPause() {
  if (!presenting()) return;
  if (timerPaused) {
    presentStart = Date.now() - pausedElapsedMs;
    timerPaused = false;
    $("btn-timer-pause").textContent = "Pause";
  } else {
    pausedElapsedMs = Date.now() - presentStart;
    timerPaused = true;
    $("btn-timer-pause").textContent = "Resume";
  }
  tickTimer();
}
function restartTimer() {
  if (!presenting()) return;
  startTimer();
}
$("btn-timer-pause").addEventListener("click", toggleTimerPause);
$("btn-timer-restart").addEventListener("click", restartTimer);
$("target-input").addEventListener("change", () => {
  const v = parseInt($("target-input").value, 10);
  if (Number.isFinite(v) && v >= 1 && v <= 600) {
    targetMinutes = v;
    tickTimer();
  } else {
    $("target-input").value = String(targetMinutes);
  }
});

// Build stepper: each slide has a single build until the M8 animation
// engine lands; the label and dots track the state the timeline will own.
const BUILDS_PER_SLIDE = 1;
function updateBuildStepper() {
  const cur = 1, total = BUILDS_PER_SLIDE;
  $("build-label").textContent = "Build " + cur + " of " + total + " — Ready · Next to continue";
  const dots = $("build-dots");
  dots.innerHTML = "";
  for (let b = 1; b <= total; b++) {
    const d = document.createElement("span");
    d.className = "build-dot" + (b <= cur ? " done" : "");
    dots.appendChild(d);
  }
}
function updateConsole() {
  if (!model) return;
  $("console-notes").textContent = currentSlide >= 0 ? (model.slides[currentSlide].notes || "") : "";
  if (currentSlide >= 0) {
    $("current-sub").textContent = "Slide " + (currentSlide + 1) + " of " + model.slides.length;
    $("ps-left").textContent =
      "Slide " + (currentSlide + 1) + " of " + model.slides.length + " · Build 1 of " + BUILDS_PER_SLIDE;
  } else {
    $("current-sub").textContent = "—";
    $("ps-left").textContent = "—";
  }
  if (currentSlide < 0) { $("next-sub").textContent = "—"; return; }
  const next = currentSlide + 1;
  if (next < model.slides.length) {
    const t = model.slides[next].title;
    $("next-sub").textContent = "Slide " + (next + 1) + " of " + model.slides.length + (t ? " — " + t : "");
    fetchSlideContent(next);
    const cached = slideContents.get(next);
    if (cached) renderSlideInto($("next-thumb"), cached, true);
  } else {
    $("next-sub").textContent = "End of deck";
  }
  updateBuildStepper();
}
// ---------- bottom slide navigator (present mode) ----------
function renderNavigator() {
  const cells = $("nav-cells");
  cells.innerHTML = "";
  if (!model) return;
  model.slides.forEach((s, i) => {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.className = "nav-cell" + (i === currentSlide ? " current" : "");
    cell.title = "Slide " + (i + 1) + (s.title ? " — " + s.title : "");
    const thumb = document.createElement("div");
    thumb.className = "nav-thumb";
    const idx = document.createElement("span");
    idx.className = "nav-idx";
    idx.textContent = String(i + 1);
    thumb.appendChild(idx);
    const cap = document.createElement("span");
    cap.className = "nav-cap";
    cap.textContent = s.title || "(no title)";
    cell.append(thumb, cap);
    cell.addEventListener("click", () => selectSlide(i));
    cells.appendChild(cell);
    const cached = slideContents.get(i);
    if (cached) renderSlideInto(thumb, cached, true);
  });
  const active = cells.querySelector(".nav-cell.current");
  if (active) active.scrollIntoView({ inline: "nearest", block: "nearest" });
}
function refreshNavigatorActive() {
  document.querySelectorAll("#nav-cells .nav-cell").forEach((el, j) => {
    el.classList.toggle("current", j === currentSlide);
  });
  const active = document.querySelector("#nav-cells .nav-cell.current");
  if (active) active.scrollIntoView({ inline: "nearest", block: "nearest" });
}
// Notes font zoom: A- / A / A+ in the console notes card.
let notesFontPx = 14;
function setNotesZoom(px) {
  notesFontPx = Math.min(24, Math.max(11, px));
  $("console-notes").style.fontSize = notesFontPx + "px";
}
$("notes-zoom-out").addEventListener("click", () => setNotesZoom(notesFontPx - 1));
$("notes-zoom-in").addEventListener("click", () => setNotesZoom(notesFontPx + 1));
$("notes-zoom-reset").addEventListener("click", () => setNotesZoom(14));
// Previous / Next buttons in the current-slide footer.
$("btn-prev").addEventListener("click", () => {
  if (model && currentSlide > 0) selectSlide(currentSlide - 1);
});
$("btn-next").addEventListener("click", () => {
  if (!model) return;
  if (currentSlide < model.slides.length - 1) selectSlide(currentSlide + 1);
  else flash("end of deck");
});
// ---------- audience display selection (top bar) ----------
async function refreshMonitorSelect() {
  const sel = $("monitor-select");
  try {
    const monitors = await invoke("list_monitors");
    sel.innerHTML = "";
    sel.disabled = !monitors.length;
    monitors.forEach((m) => {
      const opt = document.createElement("option");
      opt.value = m.name;
      opt.textContent = m.name + " · " + m.width + " × " + m.height + (m.is_primary ? " (primary)" : "");
      sel.appendChild(opt);
    });
  } catch {
    sel.disabled = true;
  }
}
function setPresentStatusRight(audienceLabel) {
  $("ps-right").textContent = "Presenting — holding notifications" + (audienceLabel ? " · Audience: " + audienceLabel : "");
}
$("monitor-select").addEventListener("change", async () => {
  const name = $("monitor-select").value;
  if (!name || !presenting()) return;
  try {
    await invoke("open_audience_window", { monitorName: name });
    setPresentStatusRight(name);
    flash("audience on " + name);
  } catch (e) { flash(String(e), "err"); }
});
$("btn-swap-displays").addEventListener("click", () => {
  const sel = $("monitor-select");
  if (sel.options.length < 2) { flash("need two displays to swap", "err"); return; }
  sel.selectedIndex = (sel.selectedIndex + 1) % sel.options.length;
  sel.dispatchEvent(new Event("change"));
});
$("btn-end-show").addEventListener("click", exitPresent);

// ---------- audience window event sync ----------
// Audience screen shutter: clear | black | white | freeze. Freeze holds the
// projected frame while the presenter browses ahead in the console.
let shutter = "clear";
function audienceFrozen() {
  return presenting() && shutter === "freeze";
}
function setShutter(mode) {
  if (!presenting() && mode !== "clear") return;
  shutter = mode;
  emitToAudience("shutter", { mode: mode === "freeze" ? "clear" : mode });
  ["black", "white", "freeze"].forEach((m) => {
    $("btn-shutter-" + m).classList.toggle("on", shutter === m);
  });
}
function toggleBlackout() {
  if (!presenting()) return;
  setShutter(shutter === "black" ? "clear" : "black");
}
$("btn-shutter-black").addEventListener("click", () => setShutter(shutter === "black" ? "clear" : "black"));
$("btn-shutter-white").addEventListener("click", () => setShutter(shutter === "white" ? "clear" : "white"));
$("btn-shutter-freeze").addEventListener("click", () => setShutter(shutter === "freeze" ? "clear" : "freeze"));
function emitToAudience(event, payload) {
  if (audienceFrozen() && (event === "slide-changed" || event === "ink" || event === "laser-move")) return;
  const ev = window.__TAURI__ && window.__TAURI__.event;
  if (ev) ev.emit(event, payload).catch(() => {});
}

// ---------- laser pointer: dot on the audience screen follows the cursor ----------
// Cursor position is normalized against the #slide-canvas rect (which keeps
// the slide's aspect ratio), so the audience side can map it back to slide
// space regardless of its own size.
let laserTool = false;
let laserCtrl = false;
let lastPointer = null;
function laserActive() { return presenting() && !inkTool && (laserTool || laserCtrl); }
function sendLaser() {
  if (!presenting()) return;
  if (!laserActive() || !lastPointer) {
    emitToAudience("laser-move", { on: false });
    return;
  }
  const rect = $("slide-canvas").getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const x = Math.min(1, Math.max(0, (lastPointer.x - rect.left) / rect.width));
  const y = Math.min(1, Math.max(0, (lastPointer.y - rect.top) / rect.height));
  emitToAudience("laser-move", { on: true, x, y });
}
function setLaserTool(on) {
  laserTool = on;
  $("btn-laser").classList.toggle("on", on);
  sendLaser();
}
document.addEventListener("pointermove", (e) => {
  lastPointer = { x: e.clientX, y: e.clientY };
  sendLaser();
});

// ---------- live ink: per-slide pen / highlighter strokes ----------
// Strokes are stored per slide as normalized (0..1) slide-space point pairs.
// The main window is the source of truth: the audience rebuilds each slide's
// ink from the slide-changed payload and appends to a live stroke as ink
// events stream in, so pen strokes appear on both screens in real time.
const INK_TOOL_STYLE = {
  pen: { color: "#0f172a", widthFrac: 0.0022, opacity: 1 },
  marker: { color: "#ffd60a", widthFrac: 0.0085, opacity: 0.45 },
};
const inkBySlide = new Map();
let inkTool = null; // "pen" | "marker" | null
let liveInk = null; // { tool, pts } while a stroke is in progress
let liveEl = null;
let drawing = false;

function inkSvg() { return $("ink-overlay"); }
function inkGroup() { return $("ink-strokes"); }

function inkNorm(e) {
  const r = $("slide-canvas").getBoundingClientRect();
  return {
    x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
    y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
  };
}

function inkPointsAttr(pts) {
  const d = slideDimensions();
  let s = "";
  for (let i = 0; i < pts.length; i += 2) {
    s += (pts[i] * d.width_emu).toFixed(1) + "," + (pts[i + 1] * d.height_emu).toFixed(1) + " ";
  }
  return s.trim();
}

function inkStrokeEl(stroke) {
  const st = INK_TOOL_STYLE[stroke.tool];
  const d = slideDimensions();
  const el = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
  el.setAttribute("fill", "none");
  el.setAttribute("stroke", st.color);
  el.setAttribute("stroke-width", (st.widthFrac * d.width_emu).toFixed(0));
  el.setAttribute("opacity", st.opacity);
  el.setAttribute("stroke-linecap", "round");
  el.setAttribute("stroke-linejoin", "round");
  el.setAttribute("points", inkPointsAttr(stroke.pts));
  return el;
}

function syncInkButtons() {
  const n = (inkBySlide.get(currentSlide) || []).length;
  $("btn-ink-undo").disabled = n === 0;
  $("btn-ink-clear").disabled = n === 0;
}

function renderInkForCurrent() {
  const d = slideDimensions();
  inkSvg().setAttribute("viewBox", `0 0 ${d.width_emu} ${d.height_emu}`);
  const g = inkGroup();
  g.innerHTML = "";
  for (const s of inkBySlide.get(currentSlide) || []) g.appendChild(inkStrokeEl(s));
  syncInkButtons();
}

function currentInk() {
  if (!inkBySlide.has(currentSlide)) inkBySlide.set(currentSlide, []);
  return inkBySlide.get(currentSlide);
}

function setInkTool(tool) {
  if (inkTool === tool) tool = null;
  inkTool = tool;
  $("btn-pen").classList.toggle("on", inkTool === "pen");
  $("btn-marker").classList.toggle("on", inkTool === "marker");
  const svg = inkSvg();
  svg.style.pointerEvents = inkTool ? "all" : "none";
  svg.classList.toggle("drawing", !!inkTool);
  if (inkTool) laserCtrl = false;
  sendLaser();
}

function endInkStroke() {
  if (!drawing) return;
  drawing = false;
  currentInk().push(liveInk);
  liveInk = null;
  liveEl = null;
  renderInkForCurrent();
  emitToAudience("ink", { op: "end" });
}

function inkUndo() {
  const list = currentInk();
  if (!list.length) return;
  list.pop();
  renderInkForCurrent();
  emitToAudience("ink", { op: "undo" });
}

function inkClear() {
  inkBySlide.delete(currentSlide);
  renderInkForCurrent();
  emitToAudience("ink", { op: "clear" });
}

function resetInk() {
  endInkStroke();
  inkBySlide.clear();
  liveInk = null;
  liveEl = null;
  if (inkTool) setInkTool(inkTool);
  renderInkForCurrent();
}

inkSvg().addEventListener("pointerdown", (e) => {
  if (!presenting() || !inkTool) return;
  e.preventDefault();
  inkSvg().setPointerCapture(e.pointerId);
  const p = inkNorm(e);
  drawing = true;
  liveInk = { tool: inkTool, pts: [p.x, p.y] };
  liveEl = inkStrokeEl(liveInk);
  inkGroup().appendChild(liveEl);
  emitToAudience("ink", { op: "begin", tool: inkTool });
  emitToAudience("ink", { op: "point", x: p.x, y: p.y });
});
inkSvg().addEventListener("pointermove", (e) => {
  if (!drawing || !liveInk) return;
  const p = inkNorm(e);
  liveInk.pts.push(p.x, p.y);
  liveEl.setAttribute("points", inkPointsAttr(liveInk.pts));
  emitToAudience("ink", { op: "point", x: p.x, y: p.y });
});
inkSvg().addEventListener("pointerup", endInkStroke);
inkSvg().addEventListener("pointercancel", endInkStroke);

$("btn-grid").onclick = toggleSlideGrid;
$("btn-pen").onclick = () => setInkTool("pen");
$("btn-marker").onclick = () => setInkTool("marker");
$("btn-ink-undo").onclick = inkUndo;
$("btn-ink-clear").onclick = inkClear;

async function startPresentation() {
  if (!model || currentSlide < 0) { flash("open a deck first", "err"); return; }
  enterPresent();
  await refreshMonitorSelect();
  try {
    const name = await invoke("open_audience_window", { monitorName: null });
    setPresentStatusRight(name);
    flash("projecting on " + name);
    emitToAudience("slide-changed", { index: currentSlide });
    setTimeout(() => emitToAudience("slide-changed", { index: currentSlide }), 200);
    setTimeout(() => emitToAudience("slide-changed", { index: currentSlide }), 600);
  } catch (e) {
    setPresentStatusRight(null);
    flash(String(e), "err");
  }
}

if (window.__TAURI__ && window.__TAURI__.event) {
  window.__TAURI__.event.listen("present-exit", () => exitPresent()).catch(() => {});
  window.__TAURI__.event.listen("audience-ready", () => {
    if (currentSlide >= 0) emitToAudience("slide-changed", { index: currentSlide });
  }).catch(() => {});
  window.__TAURI__.event.listen("present-next", () => {
    if (model && currentSlide < model.slides.length - 1) selectSlide(currentSlide + 1);
  }).catch(() => {});
  window.__TAURI__.event.listen("present-prev", () => {
    if (model && currentSlide > 0) selectSlide(currentSlide - 1);
  }).catch(() => {});
}
function nudgePresentChrome() {
  if (!presenting()) return;
  document.body.classList.add("chrome-visible");
  clearTimeout(presentHideTimer);
  presentHideTimer = setTimeout(() => document.body.classList.remove("chrome-visible"), 2200);
}
document.addEventListener("mousemove", nudgePresentChrome);
$("slide-canvas").addEventListener("click", () => {
  if (!presenting()) return;
  if (currentSlide < model.slides.length - 1) selectSlide(currentSlide + 1);
  else exitPresent();
});

function fetchSlideContent(i, force = false) {
  if (!model || i < 0 || i >= model.slides.length) return;
  if (!force && slideContents.has(i)) return;
  invoke("get_slide_content", { slide: i })
    .then((content) => {
      slideContents.set(i, content);
      paintSlide(i, content);
    })
    .catch(() => {});
}

function refreshThumbs() {
  slideContents.clear();
  hiddenShapes.clear();
  selectedShape.clear();
  if (!model) return;
  model.slides.forEach((_, i) => fetchSlideContent(i));
}

function paintSlide(i, content) {
  const item = document.querySelectorAll(".slide-item")[i];
  if (item) renderSlideInto(item.querySelector(".slide-thumb"), content, true);
  const gridThumb = document.querySelectorAll("#grid-cells .grid-thumb")[i];
  if (gridThumb) renderSlideInto(gridThumb, content, true);
  const navThumb = document.querySelectorAll("#nav-cells .nav-thumb")[i];
  if (navThumb) renderSlideInto(navThumb, content, true);
  const sorterCard = document.querySelectorAll("#sorter-body .sorter-card")[i];
  if (sorterCard) renderSlideInto(sorterCard.querySelector(".sorter-thumb"), content, true);
  if (i === currentSlide) {
    fitCanvas();
    renderSlideInto($("slide-stage"), content, false);
    applyHiddenShapes(i);
    renderLayersPanel();
  }
  if (i === currentSlide + 1) renderSlideInto($("next-thumb"), content, true);
  updateInspector();
}

// ---------- inspector: read-only slide facts ----------
function countShapes(shapes) {
  let n = 0;
  for (const s of shapes || []) { n += 1; if (s.children) n += countShapes(s.children); }
  return n;
}

function setIns(id, val) {
  const el = $(id);
  el.textContent = val;
  el.title = val;
}
function updateInspector() {
  const d = slideDimensions();
  $("insp-size").textContent = (d.width_emu / 914400).toFixed(2) + " × " + (d.height_emu / 914400).toFixed(2) + " in";
  if (!model || currentSlide < 0) {
    setIns("insp-pos", "–");
    setIns("insp-shapes", "–");
    setIns("insp-title", "–");
    return;
  }
  setIns("insp-pos", (currentSlide + 1) + " / " + model.slides.length);
  setIns("insp-title", model.slides[currentSlide].title || "—");
  const content = slideContents.get(currentSlide);
  setIns("insp-shapes", content ? String(countShapes(content.shapes)) : "…");
}

function paintCurrentSlide() {
  if (currentSlide < 0) return;
  const cached = slideContents.get(currentSlide);
  if (cached) paintSlide(currentSlide, cached);
  else fetchSlideContent(currentSlide);
}

// ---------- filmstrip ----------
let dragFrom = -1;
function renderFilmstrip() {
  const list = $("slides");
  list.innerHTML = "";
  if (!model) return;
  model.slides.forEach((s, i) => {
    const item = document.createElement("div");
    item.className = "slide-item" + (i === currentSlide ? " active" : "");

    const thumb = document.createElement("div");
    thumb.className = "slide-thumb";

    const del = document.createElement("button");
    del.className = "slide-del";
    del.textContent = "\u00d7";
    del.title = "Delete slide";
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      invoke("delete_slide", { slide: i })
        .then((m) => {
          model = JSON.parse(m);
          currentSlide = Math.min(currentSlide, model.slides.length - 1);
          renderFilmstrip();
          refreshThumbs();
          updatePreview();
          flash("slide deleted");
        })
        .catch((err) => flash(String(err), "err"));
    });

    thumb.append(del);

    const cap = document.createElement("div");
    cap.className = "slide-cap";
    const idx = document.createElement("span");
    idx.className = "slide-idx";
    idx.textContent = i + 1;

    const input = document.createElement("input");
    input.className = "slide-title";
    input.value = s.title || "";
    input.placeholder = "(no title)";
    input.addEventListener("click", (e) => e.stopPropagation());
    input.addEventListener("focus", () => selectSlide(i));
    const commit = debounce(() => {
      if (input.value === (s.title || "")) return;
      invoke("set_title", { slide: i, title: input.value })
        .then((m) => {
          model = JSON.parse(m);
          renderFilmstrip();
          if (i === currentSlide) updatePreview();
          fetchSlideContent(i, true);
          flash("title saved");
          markState("edited");
          refreshUndoUI();
        })
        .catch((e) => flash(String(e), "err"));
    }, 400);
    input.addEventListener("input", commit);
    input.addEventListener("blur", () => commit());

    cap.append(idx, input);
    item.append(thumb, cap);
    item.addEventListener("click", () => selectSlide(i));

    // drag & drop reorder: grab a thumb, drop it on another slide
    item.draggable = true;
    item.addEventListener("dragstart", (e) => {
      if (e.target.closest("input,button")) { e.preventDefault(); return; }
      dragFrom = i;
      e.dataTransfer.effectAllowed = "move";
      item.classList.add("dragging");
    });
    item.addEventListener("dragend", () => {
      dragFrom = -1;
      item.classList.remove("dragging");
      list.querySelectorAll(".drag-over").forEach((el) => el.classList.remove("drag-over"));
    });
    item.addEventListener("dragover", (e) => {
      if (dragFrom < 0 || dragFrom === i) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      item.classList.add("drag-over");
    });
    item.addEventListener("dragleave", () => item.classList.remove("drag-over"));
    item.addEventListener("drop", (e) => {
      e.preventDefault();
      if (dragFrom < 0 || dragFrom === i) return;
      const from = dragFrom;
      dragFrom = -1;
      item.classList.remove("drag-over");
      const follow = currentSlide === from;
      invoke("move_slide", { from, to: i })
        .then((m) => {
          if (follow) currentSlide = i;
          model = JSON.parse(m);
          renderFilmstrip();
          refreshThumbs();
          paintCurrentSlide();
          updatePreview();
          refreshUndoUI();
          markState("edited");
          flash("slide moved");
        })
        .catch((err) => flash(String(err), "err"));
    });
    list.appendChild(item);

    const cached = slideContents.get(i);
    if (cached) renderSlideInto(thumb, cached, true);
  });
  renderNavigator();
}

// ---------- selection ----------
function updatePreview() {
  if (!model || currentSlide < 0 || currentSlide >= model.slides.length) return;
  const notes = $("notes-input");
  if (document.activeElement !== notes) {
    notes.value = model.slides[currentSlide].notes || "";
  }
  updateNotesCount();
  $("status-slide").textContent = (currentSlide + 1) + " / " + model.slides.length;
}

function clearSlideCounter() {
  $("status-slide").textContent = "";
}

function selectSlide(i) {
  if (i === currentSlide) { updatePreview(); return; }
  if (drawing) endInkStroke();
  currentSlide = i;
  if (i >= 0) invoke("set_current_slide", { slide: i }).catch(() => {});
  document.querySelectorAll(".slide-item").forEach((el, j) => {
    el.classList.toggle("active", j === i);
  });
  document.querySelectorAll("#grid-cells .grid-cell").forEach((el, j) => {
    el.classList.toggle("current", j === i);
  });
  refreshNavigatorActive();
  const active = document.querySelectorAll(".slide-item")[i];
  if (active) active.scrollIntoView({ block: "nearest" });
  paintCurrentSlide();
  updatePreview();
  updateConsole();
  renderInkForCurrent();
  emitToAudience("slide-changed", { index: i, ink: inkBySlide.get(i) || [] });
}

function applyModel(data, selectLast = false) {
  model = typeof data === "string" ? JSON.parse(data) : data;
  $("doc-title").textContent = model.title || "";
  markState("");
  if (selectLast) currentSlide = model.slides.length - 1;
  if (currentSlide >= model.slides.length) currentSlide = model.slides.length - 1;
  if (currentSlide >= 0) invoke("set_current_slide", { slide: currentSlide }).catch(() => {});
  renderFilmstrip();
  refreshThumbs();
  paintCurrentSlide();
  if (currentSlide < 0) clearSlideCounter();
  else updatePreview();
  updateConsole();
  hasDeck();
  refreshUndoUI();
  resetInk();
}

// ---------- actions ----------
$("btn-new").onclick = () => {
  invoke("new_presentation")
    .then((m) => {
      currentSlide = -1;
      applyModel(m);
      setPath(null);
      flash("new deck");
    })
    .catch((e) => flash(String(e), "err"));
};

$("btn-open").onclick = async () => {
  try {
    const path = await invoke("open_file_dialog");
    if (!path) return;
    const p = await invoke("open_presentation", { path });
    currentSlide = 0;
    applyModel(p);
    setPath(path);
    flash("opened");
  } catch (e) { flash(String(e), "err"); }
};

$("btn-save").onclick = () => {
  if (!model) { flash("nothing to save", "err"); return; }
  invoke("save_pptx")
    .then((p) => { setPath(p); markState(""); flash("saved"); })
    .catch((e) => flash(String(e), "err"));
};

$("btn-saveas").onclick = async () => {
  if (!model) { flash("nothing to save", "err"); return; }
  try {
    const path = await invoke("save_file_dialog");
    if (!path) return;
    const p = await invoke("save_as", { path });
    setPath(p);
    markState("");
    flash("saved");
  } catch (e) { flash(String(e), "err"); }
};

async function doExportPdf() {
  if (!model) { flash("nothing to export", "err"); return; }
  try {
    const path = await invoke("save_file_dialog");
    if (!path) return;
    flash("rendering PDF…");
    const p = await invoke("export_pdf", { path });
    flash(`PDF exported: ${p}`);
  } catch (e) { flash(String(e), "err"); }
}

async function doExportHtml() {
  if (!model) { flash("nothing to export", "err"); return; }
  try {
    const path = await invoke("save_file_dialog");
    if (!path) return;
    flash("rendering HTML bundle…");
    const p = await invoke("export_html", { path });
    flash(`HTML exported: ${p}`);
  } catch (e) { flash(String(e), "err"); }
}

$("btn-exportpdf").onclick = doExportPdf;
$("btn-exporthtml").onclick = doExportHtml;
$("panel-export-pdf").onclick = doExportPdf;
$("panel-export-html").onclick = doExportHtml;

// ---------- sorter: deck grid, sections, batch reorder, themes ----------
const SORTER_THEMES = {
  green:  { accent: "#7fb069", accent2: "#a3c585", accent3: "#d4e8b8" },
  blue:   { accent: "#3d8bfd", accent2: "#7fb2f7", accent3: "#b8d4f7" },
  sunset: { accent: "#e07b39", accent2: "#f2a65a", accent3: "#f7d4b8" },
  mono:   { accent: "#9aa3b5", accent2: "#c2c9d6", accent3: "#e2e6ee" },
};
// Sections partition the deck by counts (session-local, never written to the
// PPTX): the i-th section spans slides [sum(counts[0..i)), +count[i]).
let sorterSections = [];
let sorterSel = new Set();
let sorterTheme = "none";
let sorterThemeApplied = false;

function enterSorter() {
  if (!model || currentSlide < 0) { flash("open a deck first", "err"); return; }
  const n = model.slides.length;
  const total = sorterSections.reduce((a, s) => a + s.count, 0);
  if (!sorterSections.length || total !== n) {
    sorterSections = [{ name: "All slides", count: n, collapsed: false }];
  }
  sorterSel = new Set();
  sorterTheme = "none";
  sorterThemeApplied = false;
  if (window.themeRemap) { window.themeRemap = null; }
  document.querySelectorAll("#sorter-themes .theme-swatch").forEach((b) =>
    b.classList.toggle("on", b.dataset.theme === "none"));
  refreshThumbs();
  renderSorter();
}

function renderSorter() {
  const body = $("sorter-body");
  if (!model) return;
  body.innerHTML = "";
  $("sorter-count").textContent = "Sorter — " + model.slides.length + " slides";
  let start = 0;
  sorterSections.forEach((sec, si) => {
    const end = start + sec.count;
    const secEl = document.createElement("div");
    secEl.className = "sorter-section" + (sec.collapsed ? " collapsed" : "");

    const head = document.createElement("div");
    head.className = "sorter-sec-head";
    const chev = document.createElement("button");
    chev.className = "sec-chev";
    chev.textContent = "\u25be";
    chev.title = sec.collapsed ? "Expand section" : "Collapse section";
    chev.onclick = () => { sec.collapsed = !sec.collapsed; renderSorter(); };
    const name = document.createElement("span");
    name.className = "sec-name";
    name.textContent = sec.name;
    name.title = "Double-click to rename";
    name.ondblclick = () => renameSection(sec, name);
    const range = document.createElement("span");
    range.className = "sec-range";
    range.textContent = sec.count ? (start + 1) + "\u2013" + end : "(empty)";
    const del = document.createElement("button");
    del.className = "sec-del";
    del.textContent = "\u00d7";
    del.title = "Delete section — its slides merge into the previous one";
    del.onclick = () => deleteSection(si);
    head.append(chev, name, range, del);

    const grid = document.createElement("div");
    grid.className = "sorter-grid";
    for (let i = start; i < end; i++) buildSorterCard(grid, i);
    secEl.append(head, grid);
    body.appendChild(secEl);
    start = end;
  });
  repaintSorterThumbs();
}

function buildSorterCard(grid, i) {
  const s = model.slides[i];
  const card = document.createElement("div");
  card.className = "sorter-card" + (sorterSel.has(i) ? " sel" : "");
  card.dataset.idx = i;
  card.draggable = true;

  const thumb = document.createElement("div");
  thumb.className = "sorter-thumb";
  const cap = document.createElement("div");
  cap.className = "sorter-cap";
  const num = document.createElement("span");
  num.className = "num";
  num.textContent = i + 1;
  const ttl = document.createElement("span");
  ttl.className = "ttl";
  ttl.textContent = s.title || "(no title)";
  const mvL = document.createElement("button");
  mvL.className = "mv-btn";
  mvL.textContent = "\u25c0";
  mvL.title = "Move slide left";
  const mvR = document.createElement("button");
  mvR.className = "mv-btn";
  mvR.textContent = "\u25b6";
  mvR.title = "Move slide right";
  cap.append(num, ttl, mvL, mvR);
  card.append(thumb, cap);

  card.addEventListener("click", (e) => {
    if (e.target.closest("button")) return;
    if (e.ctrlKey || e.metaKey) {
      if (sorterSel.has(i)) sorterSel.delete(i);
      else sorterSel.add(i);
    } else {
      sorterSel = new Set([i]);
    }
    renderSorter();
  });

  card.addEventListener("dragstart", (e) => {
    if (!sorterSel.has(i)) sorterSel = new Set([i]);
    card.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", String(i));
  });
  card.addEventListener("dragend", clearSorterDropMarks);
  card.addEventListener("dragover", (e) => {
    e.preventDefault();
    const r = card.getBoundingClientRect();
    const before = e.clientX - r.left < r.width / 2;
    clearSorterDropMarks();
    card.classList.add(before ? "drop-before" : "drop-after");
  });
  card.addEventListener("drop", (e) => {
    e.preventDefault();
    const r = card.getBoundingClientRect();
    const before = e.clientX - r.left < r.width / 2;
    sorterMoveSelectionTo(before ? i : i + 1);
  });

  mvL.addEventListener("click", (e) => { e.stopPropagation(); sorterMoveSlideBy(i, -1); });
  mvR.addEventListener("click", (e) => { e.stopPropagation(); sorterMoveSlideBy(i, 1); });

  grid.appendChild(card);
}

function clearSorterDropMarks() {
  document.querySelectorAll("#sorter-body .drop-before,#sorter-body .drop-after").forEach((c) =>
    c.classList.remove("drop-before", "drop-after"));
}

function repaintSorterThumbs() {
  for (const [i, c] of slideContents) paintSlide(i, c);
}

// Reorder the whole deck to `order` (a permutation of 0..n) in one undoable
// step. The selection follows its slides across the move.
function sorterReorderTo(order) {
  const selBefore = Array.from(sorterSel).sort((a, b) => a - b);
  invoke("reorder_slides", { order })
    .then((m) => {
      model = JSON.parse(m);
      sorterSel = new Set(selBefore.map((k) => order.indexOf(k)));
      sorterReconcileSections(order, selBefore);
      slideContents.clear();
      refreshThumbs();
      renderSorter();
      if (currentSlide >= model.slides.length) currentSlide = model.slides.length - 1;
      markState("edited");
      refreshUndoUI();
      flash("slides reordered");
    })
    .catch((err) => { renderSorter(); flash(String(err), "err"); });
}

function sorterMoveSelectionTo(target) {
  if (!model || !sorterSel.size) return;
  const n = model.slides.length;
  const rest = Array.from({ length: n }, (_, k) => k).filter((k) => !sorterSel.has(k));
  const sel = Array.from(sorterSel).sort((a, b) => a - b);
  const t = Math.max(0, Math.min(target, rest.length));
  rest.splice(t, 0, ...sel);
  sorterReorderTo(rest);
}

function sorterMoveSlideBy(i, dir) {
  if (!model) return;
  const n = model.slides.length;
  const j = i + dir;
  if (j < 0 || j >= n) return;
  const order = Array.from({ length: n }, (_, k) => k);
  order.splice(i, 1);
  // j is the target index in the final array; after the removal the insert
  // position is j itself (both directions).
  order.splice(j, 0, i);
  sorterSel = new Set([i]);
  sorterReorderTo(order);
}

// Keep the section partition valid after a move: counts follow the slides.
function sorterReconcileSections(order, selBefore) {
  if (sorterSections.length <= 1) return;
  const counts = sorterSections.map((s) => s.count);
  const secOf = (idx) => {
    let s = 0;
    for (let si = 0; si < counts.length; si++) {
      s += counts[si];
      if (idx < s) return si;
    }
    return counts.length - 1;
  };
  for (const k of selBefore) {
    const from = secOf(k);
    const to = secOf(order.indexOf(k));
    if (from !== to) { counts[from]--; counts[to]++; }
  }
  sorterSections.forEach((s, i) => { s.count = counts[i]; });
}

function renameSection(sec, nameEl) {
  const input = document.createElement("input");
  input.className = "sec-rename";
  input.value = sec.name;
  nameEl.replaceWith(input);
  input.focus();
  input.select();
  let done = false;
  const commit = () => {
    if (done) return;
    done = true;
    const v = input.value.trim();
    if (v) sec.name = v;
    renderSorter();
  };
  input.addEventListener("blur", commit);
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") input.blur();
    else if (e.key === "Escape") { e.preventDefault(); done = true; renderSorter(); }
  });
}

function deleteSection(si) {
  if (sorterSections.length <= 1) { flash("cannot delete the only section", "err"); return; }
  const gone = sorterSections.splice(si, 1)[0];
  sorterSections[si > 0 ? si - 1 : 0].count += gone.count;
  renderSorter();
}

function addSorterSection() {
  if (!model) return;
  const n = model.slides.length;
  let splitAt = sorterSel.size ? Math.max(...Array.from(sorterSel)) + 1 : Math.floor(n / 2);
  splitAt = Math.max(1, Math.min(splitAt, n - 1));
  let s = 0;
  for (let si = 0; si < sorterSections.length; si++) {
    const c = sorterSections[si].count;
    if (splitAt - 1 < s + c) {
      const right = splitAt - s;
      sorterSections[si].count = c - right;
      sorterSections.splice(si + 1, 0, { name: "Section " + (si + 2), count: right, collapsed: false });
      break;
    }
    s += c;
  }
  renderSorter();
}

// ---------- theme engine ----------
function cssHexToRgb(h) {
  const n = parseInt(h.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
}
function isNeutralColor(c) {
  if (!/^#[0-9a-f]{6}$/i.test(c)) return true;
  const [r, g, b] = cssHexToRgb(c);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lum = (max + min) / 2;
  const sat = max === min ? 0 : (max - min) / (1 - Math.abs(2 * lum - 1));
  return sat < 0.12 || lum < 0.05 || lum > 0.97;
}

// The deck's three most frequent saturated colors, in frequency order.
function computeDominantColors() {
  const score = new Map();
  const bump = (c) => {
    if (!c || !/^#[0-9a-f]{6}$/i.test(c) || isNeutralColor(c)) return;
    const k = c.toLowerCase();
    score.set(k, (score.get(k) || 0) + 1);
  };
  const walk = (shapes) => {
    for (const s of shapes || []) {
      bump(s.fill);
      if (s.line && s.line.color) bump(s.line.color);
      for (const r of s.runs || []) bump(r.color);
      if (s.children) walk(s.children);
    }
  };
  for (const c of slideContents.values()) walk(c.shapes);
  return Array.from(score.entries()).sort((a, b) => b[1] - a[1]).slice(0, 3).map(([k]) => k);
}

function setSorterTheme(name) {
  sorterTheme = name;
  document.querySelectorAll("#sorter-themes .theme-swatch").forEach((b) =>
    b.classList.toggle("on", b.dataset.theme === name));
  if (name === "none") window.themeRemap = null;
  else {
    const t = SORTER_THEMES[name];
    const dom = computeDominantColors();
    // themeColor() in slide_render.js looks up bare lowercase hex, no "#".
    const bare = (c) => c.replace(/^#/, "").toLowerCase();
    const remap = {};
    if (dom[0]) remap[bare(dom[0])] = t.accent;
    if (dom[1]) remap[bare(dom[1])] = t.accent2;
    if (dom[2]) remap[bare(dom[2])] = t.accent3;
    window.themeRemap = remap;
  }
  repaintSorterThumbs();
  paintCurrentSlide();
}

function applyThemeToDeck() {
  if (sorterTheme === "none" || !model) { flash("pick a theme first", "err"); return; }
  const t = SORTER_THEMES[sorterTheme];
  const dom = computeDominantColors();
  const map = [];
  // the core validates bare 6-digit hex (no "#").
  if (dom[0]) map.push([dom[0].replace(/^#/, "").toLowerCase(), t.accent]);
  if (dom[1]) map.push([dom[1].replace(/^#/, "").toLowerCase(), t.accent2]);
  if (dom[2]) map.push([dom[2].replace(/^#/, "").toLowerCase(), t.accent3]);
  if (!map.length) { flash("no dominant colors found in this deck", "err"); return; }
  invoke("apply_theme", { map })
    .then((m) => {
      model = JSON.parse(m);
      sorterThemeApplied = true;
      markState("edited");
      slideContents.clear();
      refreshThumbs();
      paintCurrentSlide();
      flash("theme applied — saved to file");
    })
    .catch((err) => flash(String(err), "err"));
}

$("btn-sorter-add").onclick = addSorterSection;
$("btn-sorter-apply").onclick = applyThemeToDeck;
$("btn-sorter-done").onclick = () => {
  if (sorterTheme !== "none" && !sorterThemeApplied) applyThemeToDeck();
  enterMode("edit");
};
document.querySelectorAll("#sorter-themes .theme-swatch").forEach((b) => {
  b.onclick = () => setSorterTheme(b.dataset.theme);
});

// ---------- top mode bar (Alt+1…Alt+7) ----------
const MODE_ORDER = ["edit", "design", "animate", "review", "present", "export", "sorter"];
const MODE_COPY = {
  design: "Masters, layouts, palettes and typography controls land in the next step of this milestone. Until then, the Edit canvas renders the deck's current master and layout exactly.",
  animate: "Build-in effects, easing curves and the timeline land in the next step of this milestone. Slide-to-slide transitions already run in Present mode.",
  review: "Comments, change tracking and revision history land later. For now, walk the deck in Present mode and check each shape in the Edit inspectors.",
};
function setModeTab(m) {
  document.querySelectorAll("#modebar .mode").forEach((b) => b.classList.toggle("on", b.dataset.mode === m));
}
function enterMode(m) {
  // The console owns the layout while presenting; only PRESENT re-enters it.
  if (presenting() && m !== "present") return;
  if (m === "present") {
    if (!model || currentSlide < 0) { flash("open a deck first", "err"); return; }
    setModeTab("present");
    startPresentation();
    return;
  }
  if (m === "sorter") {
    if (!model || currentSlide < 0) { flash("open a deck first", "err"); return; }
    document.body.dataset.mode = "sorter";
    setModeTab("sorter");
    enterSorter();
    return;
  }
  // Leaving the sorter: an unapplied theme preview dies with the mode.
  const wasSorter = document.body.dataset.mode === "sorter";
  if (wasSorter && !sorterThemeApplied && window.themeRemap) {
    window.themeRemap = null;
  }
  if (m !== "edit" && m !== "export") {
    $("ph-title").textContent = m[0].toUpperCase() + m.slice(1);
    $("ph-body").textContent = MODE_COPY[m] || "";
  }
  document.body.dataset.mode = m;
  setModeTab(m);
  if (wasSorter) { fitCanvas(); paintCurrentSlide(); }
}
function resetModeToEdit() {
  document.body.dataset.mode = "edit";
  setModeTab("edit");
}
document.querySelectorAll("#modebar .mode").forEach((b) => {
  b.onclick = () => enterMode(b.dataset.mode);
});
document.addEventListener("keydown", (e) => {
  if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
  if (!/^Digit[1-7]$/.test(e.code)) return;
  const t = e.target;
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
  e.preventDefault();
  enterMode(MODE_ORDER[Number(e.code.slice(5)) - 1]);
});

const zoomRange = $("zoom-range");
const zoomPct = $("zoom-pct");
function syncZoomUI() {
  if (zoomMode === "fit") {
    zoomPct.textContent = lastFitPct ? lastFitPct + "%" : "Fit";
    zoomRange.value = "100";
  } else zoomPct.textContent = zoomMode + "%";
  $("zoom-fit").classList.toggle("on", zoomMode === "fit");
  $("zoom-100").classList.toggle("on", zoomMode === "100");
}
$("zoom-fit").onclick = () => { setZoom("fit"); syncZoomUI(); };
$("zoom-100").onclick = () => { setZoom("100"); syncZoomUI(); };
zoomRange.addEventListener("input", () => { setZoom(zoomRange.value); syncZoomUI(); });
$("notes-label").onclick = () => $("notes-bar").classList.toggle("collapsed");
$("btn-present").onclick = startPresentation;
$("btn-laser").onclick = () => setLaserTool(!laserTool);
const btnEnd = $("btn-end-show");
if (btnEnd) btnEnd.onclick = exitPresent;
syncZoomUI();
const addSlide = () => {
  const idx = Math.max(0, currentSlide + 1);
  invoke("add_slide_at", { index: idx, title: null })
    .then((m) => {
      applyModel(m);
      currentSlide = idx;
      renderFilmstrip();
      markState("edited");
      flash("slide added");
      const item = document.querySelectorAll(".slide-item")[idx];
      if (item) item.querySelector("input").focus();
    })
    .catch((e) => flash(String(e), "err"));
};
$("btn-add").onclick = addSlide;

// ---------- undo / redo ----------
function refreshUndoUI() {
  const u = $("btn-undo"), r = $("btn-redo");
  if (!model) { u.disabled = true; r.disabled = true; return; }
  invoke("undo_state")
    .then((s) => {
      u.disabled = !s.can_undo;
      u.title = s.can_undo ? "Undo: " + s.last + " (Ctrl+Z)" : "Nothing to undo";
      r.disabled = !s.can_redo;
    })
    .catch(() => {});
}
function doUndo() {
  invoke("undo_presentation")
    .then((m) => { applyModel(m); markState("edited"); flash("undone"); })
    .catch((e) => flash(String(e), "err"));
}
function doRedo() {
  invoke("redo_presentation")
    .then((m) => { applyModel(m); markState("edited"); flash("redone"); })
    .catch((e) => flash(String(e), "err"));
}
$("btn-undo").onclick = doUndo;
$("btn-redo").onclick = doRedo;

// ---------- text editing: double-click a shape to edit its text ----------
function findShapeById(shapes, id) {
  for (const s of shapes) {
    if (s.id === id) return s;
    if (s.children) {
      const hit = findShapeById(s.children, id);
      if (hit) return hit;
    }
  }
  return null;
}
const textEdit = $("text-edit");
let textEditCtx = null;
function openTextEdit(el) {
  if (presenting() || !model) return;
  const shapeId = Number(el.dataset.shapeId);
  if (!shapeId) return;
  const content = slideContents.get(currentSlide);
  const shape = content && findShapeById(content.shapes, shapeId);
  const initial = shape && shape.text != null ? shape.text : el.innerText;
  const canvas = $("slide-canvas").getBoundingClientRect();
  const r = el.getBoundingClientRect();
  textEditCtx = { slide: currentSlide, shapeId, initial };
  textEdit.value = initial;
  textEdit.style.left = (r.left - canvas.left) + "px";
  textEdit.style.top = (r.top - canvas.top) + "px";
  textEdit.style.width = r.width + "px";
  textEdit.style.height = r.height + "px";
  textEdit.style.fontSize = getComputedStyle(el).fontSize;
  textEdit.style.display = "block";
  textEdit.focus();
  textEdit.select();
}
function closeTextEdit(commit) {
  if (!textEditCtx) return;
  const ctx = textEditCtx;
  textEditCtx = null;
  textEdit.style.display = "none";
  const val = textEdit.value;
  if (!commit || val === ctx.initial) return;
  invoke("update_text_run", { slide: ctx.slide, shapeId: ctx.shapeId, newText: val })
    .then(() => {
      fetchSlideContent(ctx.slide, true);
      markState("edited");
      refreshUndoUI();
      flash("text updated");
    })
    .catch((e) => flash(String(e), "err"));
}
textEdit.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); closeTextEdit(true); }
  else if (e.key === "Escape") { e.preventDefault(); closeTextEdit(false); }
});
textEdit.addEventListener("blur", () => closeTextEdit(true));
$("slide-stage").addEventListener("dblclick", (e) => {
  const el = e.target.closest(".slide-shape[data-shape-id]");
  if (el) openTextEdit(el);
});

// notes editor — live, debounced
const commitNotes = debounce(() => {
  if (!model || currentSlide < 0) return;
  const val = $("notes-input").value;
  const s = model.slides[currentSlide];
  if (val === (s.notes || "")) return;
  invoke("set_notes", { slide: currentSlide, notes: val || null })
    .then((m) => {
      model = JSON.parse(m);
      markState("");
      flash("notes saved");
      updateConsole();
      refreshUndoUI();
    })
    .catch((e) => flash(String(e), "err"));
}, 500);
const notesInput = $("notes-input");
notesInput.addEventListener("input", () => {
  notesInput.style.height = "auto";
  notesInput.style.height = Math.min(notesInput.scrollHeight, 120) + "px";
  markState("edited");
  updateNotesCount();
  commitNotes();
});

// ---------- layers panel: selection, visibility, z-order ----------
const layersPanel = $("layers-panel");
const layersList = $("layers-list");
const layersCount = $("layers-count");
const layersCollapse = $("layers-collapse");

// Session-local display state per slide — intentionally not persisted to the PPTX.
const hiddenShapes = new Map();  // slide index -> Set<shapeId>
const selectedShape = new Map(); // slide index -> shapeId

const EYE_OPEN = '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12z"/><circle cx="12" cy="12" r="2.8"/></svg>';
const EYE_CLOSED = '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12z"/><circle cx="12" cy="12" r="2.8"/><line x1="4" y1="20" x2="20" y2="4"/></svg>';

function layerName(shape) {
  const t = (shape.text || "").replace(/\s+/g, " ").trim();
  if (t) return t.length > 30 ? t.slice(0, 29) + "…" : t;
  return shape.name || "Shape";
}

function applyHiddenShapes(slide) {
  const hidden = hiddenShapes.get(slide);
  const sel = "#slide-stage .slide-shape[data-shape-id],#slide-stage .slide-pic[data-shape-id],#slide-stage .slide-connector[data-shape-id]";
  document.querySelectorAll(sel).forEach((el) => {
    const id = Number(el.dataset.shapeId);
    el.style.display = (hidden && hidden.has(id)) ? "none" : "";
  });
}

function renderLayersPanel() {
  const shapes = model && currentSlide >= 0 ? (slideContents.get(currentSlide)?.shapes || []) : [];
  layersCount.textContent = shapes.length ? String(shapes.length) : "";
  if (!shapes.length) {
    layersList.innerHTML = '<div id="layers-empty">No shapes on this slide</div>';
    return;
  }
  const sel = selectedShape.get(currentSlide);
  const hidden = hiddenShapes.get(currentSlide) || new Set();
  const n = shapes.length;
  // Front of the z-stack first: document order is back-to-front, so reverse it.
  layersList.replaceChildren(...shapes.map((shape, rev) => {
    const docIdx = n - 1 - rev;
    const isHidden = hidden.has(shape.id);
    const row = document.createElement("div");
    row.className = "layer-item"
      + (shape.id === sel ? " active" : "")
      + (isHidden ? " hidden-layer" : "");
    row.dataset.shapeId = String(shape.id);
    row.title = `Layer ${docIdx + 1} of ${n} · ${shape.kind}`;

    const eye = document.createElement("button");
    eye.className = "layer-eye";
    eye.title = isHidden ? "Show shape" : "Hide shape (display only, not saved)";
    eye.innerHTML = isHidden ? EYE_CLOSED : EYE_OPEN;
    eye.addEventListener("click", (e) => {
      e.stopPropagation();
      const set = hiddenShapes.get(currentSlide) || new Set();
      if (set.has(shape.id)) set.delete(shape.id); else set.add(shape.id);
      hiddenShapes.set(currentSlide, set);
      applyHiddenShapes(currentSlide);
      renderLayersPanel();
    });

    const name = document.createElement("span");
    name.className = "layer-name";
    name.textContent = layerName(shape);

    const type = document.createElement("span");
    type.className = "layer-type";
    type.textContent = (shape.kind || "shape").toUpperCase();

    const acts = document.createElement("span");
    acts.className = "layer-actions";
    const up = document.createElement("button");
    up.className = "layer-up";
    up.title = "Bring forward (closer to viewer)";
    up.disabled = docIdx === n - 1;
    up.addEventListener("click", (e) => { e.stopPropagation(); reorderLayer(shape.id, "bring_forward"); });
    const down = document.createElement("button");
    down.className = "layer-down";
    down.title = "Send backward (further from viewer)";
    down.disabled = docIdx === 0;
    down.addEventListener("click", (e) => { e.stopPropagation(); reorderLayer(shape.id, "send_backward"); });
    acts.append(up, down);

    row.append(eye, name, type, acts);
    row.addEventListener("click", () => {
      selectedShape.set(currentSlide, shape.id);
      renderLayersPanel();
    });
    return row;
  }));
  // Re-apply the highlight to the freshly painted stage.
  if (sel != null) {
    const el = document.querySelector(`#slide-stage .slide-shape[data-shape-id="${sel}"],#slide-stage .slide-pic[data-shape-id="${sel}"],#slide-stage .slide-connector[data-shape-id="${sel}"]`);
    if (el) el.classList.add("shape-selected");
  }
}

function reorderLayer(shapeId, action) {
  if (!model || currentSlide < 0) return;
  const slide = currentSlide;
  invoke("reorder_shape_z_order", { slide, shapeId, action })
    .then((m) => {
      model = JSON.parse(m);
      fetchSlideContent(slide, true);
      markState("edited");
      refreshUndoUI();
      flash(action === "bring_forward" ? "brought forward" : "sent backward");
    })
    .catch((e) => flash(String(e), "err"));
}

// Single click on a stage shape selects it in the panel (double click still edits text).
$("slide-stage").addEventListener("click", (e) => {
  if (document.body.dataset.mode !== "edit" || inkTool) return;
  const el = e.target.closest(".slide-shape[data-shape-id],.slide-pic[data-shape-id]");
  if (el) selectedShape.set(currentSlide, Number(el.dataset.shapeId));
  else if (e.target.id === "slide-stage")
    selectedShape.delete(currentSlide);
  renderLayersPanel();
});

layersCollapse.addEventListener("click", () => {
  const collapsed = layersPanel.classList.toggle("collapsed");
  layersCollapse.innerHTML = collapsed ? "&#9656;" : "&#9662;";
  layersCollapse.title = collapsed ? "Expand layers" : "Collapse layers";
});

// ---------- keyboard: arrows switch slides, Esc ends the show ----------
document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && (e.key === "z" || e.key === "Z" || e.key === "y" || e.key === "Y")) {
    if (e.target.tagName !== "INPUT" && e.target.tagName !== "TEXTAREA") {
      e.preventDefault();
      if ((e.key.toLowerCase() === "z" && e.shiftKey) || e.key.toLowerCase() === "y") doRedo();
      else doUndo();
      return;
    }
  }
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA" || e.target.isContentEditable) return;
  if (document.body.dataset.mode === "sorter" && e.key === "Escape") {
    e.preventDefault();
    enterMode("edit");
    return;
  }
  if (presenting() && e.key === "Escape") {
    e.preventDefault();
    if (gridOpen()) closeSlideGrid();
    else exitPresent();
    return;
  }
  if (presenting() && e.key.toLowerCase() === "q") {
    e.preventDefault();
    exitPresent();
    return;
  }
  if (presenting() && e.key === "Control" && !e.repeat) {
    laserCtrl = true;
    sendLaser();
    return;
  }
  if (e.key === "F5") {
    e.preventDefault();
    if (!presenting()) startPresentation();
    return;
  }
  if (presenting() && e.key.toLowerCase() === "b") {
    e.preventDefault();
    toggleBlackout();
    return;
  }
  if (presenting() && e.key.toLowerCase() === "g") {
    e.preventDefault();
    toggleSlideGrid();
    return;
  }
  if (presenting() && e.key.toLowerCase() === "d") {
    e.preventDefault();
    setInkTool("pen");
    return;
  }
  if (presenting() && e.key.toLowerCase() === "m") {
    e.preventDefault();
    setInkTool("marker");
    return;
  }
  if (presenting() && e.key.toLowerCase() === "u") {
    e.preventDefault();
    inkUndo();
    return;
  }
  if (presenting() && e.key.toLowerCase() === "c") {
    e.preventDefault();
    inkClear();
    return;
  }
  if (!model) return;
  if (e.key === "ArrowRight" || e.key === "PageDown" || e.key === " " || e.key === "Enter") {
    if (currentSlide < model.slides.length - 1) selectSlide(currentSlide + 1);
    else if (presenting()) exitPresent();
  } else if (e.key === "ArrowLeft" || e.key === "PageUp") {
    if (currentSlide > 0) selectSlide(currentSlide - 1);
  }
});

document.addEventListener("keyup", (e) => {
  if (presenting() && e.key === "Control") {
    laserCtrl = false;
    sendLaser();
  }
});

// Re-fit the canvas and re-paint slides when the window resizes.
let resizeT = null;
window.addEventListener("resize", () => {
  clearTimeout(resizeT);
  resizeT = setTimeout(() => {
    if (!model) return;
    document.querySelectorAll(".slide-item").forEach((item, i) => {
      const cached = slideContents.get(i);
      if (cached) renderSlideInto(item.querySelector(".slide-thumb"), cached, true);
    });
    document.querySelectorAll("#nav-cells .nav-thumb").forEach((thumb, i) => {
      const cached = slideContents.get(i);
      if (cached) renderSlideInto(thumb, cached, true);
    });
    paintCurrentSlide();
  }, 200);
});

// Launched with a file argument? Open that deck once the UI is up.
invoke("initial_deck_path").then((p) => {
  if (!p) return;
  invoke("open_presentation", { path: p })
    .then((sum) => { currentSlide = 0; applyModel(sum); setPath(p); flash("opened"); })
    .catch((e) => flash(String(e), "err"));
}).catch(() => {});

hasDeck();
