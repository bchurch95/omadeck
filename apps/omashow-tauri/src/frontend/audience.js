// Audience window: slide surface on the secondary monitor (or preview window).
// Stays in sync with the presenter console via Tauri events; Esc or the
// console's present-exit event closes the window.

const invoke = (cmd, args = {}) => window.__TAURI__.core.invoke(cmd, args);
const canvas = document.getElementById("slide-canvas");
const laser = document.getElementById("laser");
const inkSvg = document.getElementById("ink-overlay");
const inkGroup = document.getElementById("ink-strokes");
const events = window.__TAURI__.event;
let slideDims = null;
let audCurrent = -1;

// Mirrors the console's INK_TOOL_STYLE — stroke width is a fraction of the
// slide width in the SVG's EMU viewBox, so both screens render identical
// relative line thickness.
const INK_TOOL_STYLE = {
  pen: { color: "#0f172a", widthFrac: 0.0022, opacity: 1 },
  marker: { color: "#ffd60a", widthFrac: 0.0085, opacity: 0.45 },
};
const audInk = new Map(); // slide index -> [{ tool, pts }]
let audLive = null; // { tool, pts, el } while a stroke is streaming in

function audPtsAttr(pts) {
  let s = "";
  for (let i = 0; i < pts.length; i += 2) {
    s += (pts[i] * slideDims.width_emu).toFixed(1) + "," + (pts[i + 1] * slideDims.height_emu).toFixed(1) + " ";
  }
  return s.trim();
}

function audStrokeEl(stroke) {
  const st = INK_TOOL_STYLE[stroke.tool];
  const el = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
  el.setAttribute("fill", "none");
  el.setAttribute("stroke", st.color);
  el.setAttribute("stroke-width", (st.widthFrac * slideDims.width_emu).toFixed(0));
  el.setAttribute("opacity", st.opacity);
  el.setAttribute("stroke-linecap", "round");
  el.setAttribute("stroke-linejoin", "round");
  el.setAttribute("points", audPtsAttr(stroke.pts));
  return el;
}

function fitAudienceCanvas(dims) {
  if (!dims) return;
  const availW = window.innerWidth;
  const availH = window.innerHeight;
  let w = availW;
  let h = (w * dims.height_emu) / dims.width_emu;
  if (h > availH) {
    h = availH;
    w = (h * dims.width_emu) / dims.height_emu;
  }
  canvas.style.width = Math.floor(w) + "px";
  canvas.style.height = Math.floor(h) + "px";
  sizeInkOverlay();
}

function sizeInkOverlay() {
  if (!slideDims) return;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  inkSvg.style.width = w + "px";
  inkSvg.style.height = h + "px";
  inkSvg.style.left = canvas.offsetLeft + "px";
  inkSvg.style.top = canvas.offsetTop + "px";
  inkSvg.setAttribute("viewBox", `0 0 ${slideDims.width_emu} ${slideDims.height_emu}`);
}

function audRenderInk(strokes) {
  if (!slideDims) return;
  sizeInkOverlay();
  inkGroup.innerHTML = "";
  for (const s of strokes || []) inkGroup.appendChild(audStrokeEl(s));
}

async function renderSlideAt(i) {
  try {
    const content = await invoke("get_slide_content", { slide: i });
    slideDims = content.slide_dimensions;
    audCurrent = i;
    fitAudienceCanvas(slideDims);
    renderSlideInto(canvas, content, false);
    audRenderInk(audInk.get(i) || []);
  } catch (err) {
    console.error("audience: failed to render slide", err);
  }
}

events.listen("slide-changed", (e) => {
  const p = e.payload;
  if (!p || typeof p.index !== "number") return;
  audInk.set(p.index, (p.ink || []).map((s) => ({ tool: s.tool, pts: [...s.pts] })));
  renderSlideAt(p.index);
});

// Pull current slide on load so audience never starts blank
invoke("get_current_slide")
  .then(renderSlideAt)
  .catch((err) => console.error("audience: no current slide", err));

events.listen("laser-move", (e) => {
  const p = e.payload;
  if (!p || !p.on) { laser.style.display = "none"; return; }
  if (!slideDims) return;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  laser.style.left = (canvas.offsetLeft + p.x * w) + "px";
  laser.style.top = (canvas.offsetTop + p.y * h) + "px";
  laser.style.display = "block";
});

events.listen("ink", (e) => {
  const p = e.payload;
  if (!p || !slideDims) return;
  if (p.op === "begin") {
    audLive = { tool: p.tool, pts: [], el: null };
    audLive.el = audStrokeEl(audLive);
    inkGroup.appendChild(audLive.el);
  } else if (p.op === "point" && audLive) {
    audLive.pts.push(p.x, p.y);
    audLive.el.setAttribute("points", audPtsAttr(audLive.pts));
  } else if (p.op === "end" && audLive) {
    if (!audInk.has(audCurrent)) audInk.set(audCurrent, []);
    audInk.get(audCurrent).push({ tool: audLive.tool, pts: audLive.pts });
    audLive = null;
  } else if (p.op === "undo") {
    const list = audInk.get(audCurrent) || [];
    list.pop();
    audLive = null;
    audRenderInk(list);
  } else if (p.op === "clear") {
    audLive = null;
    audInk.set(audCurrent, []);
    audRenderInk([]);
  }
});

// A shutter must silence embedded media, not just hide it.
function setCanvasMediaPaused(paused) {
  canvas.querySelectorAll(".slide-media video,.slide-media audio").forEach((el) => {
    if (paused) el.pause();
    else el.play().catch(() => {});
  });
}

if (events) {
  events.listen("shutter", (e) => {
    const mode = e.payload && e.payload.mode;
    const engaged = mode === "black" || mode === "white";
    canvas.classList.toggle("blackout", mode === "black");
    canvas.classList.toggle("whiteout", mode === "white");
    setCanvasMediaPaused(engaged);
  });

  events.listen("blackout-toggle", (e) => {
    canvas.classList.toggle("blackout", !!e.payload.on);
    setCanvasMediaPaused(!!e.payload.on);
  });

  events.listen("present-exit", () => {
    invoke("close_audience_window").catch(() => {});
  });

  // Announce readiness so main window emits current slide immediately
  events.emit("audience-ready", {}).catch(() => {});
}

// Keep canvas letterboxed/pillarboxed on resize
window.addEventListener("resize", () => {
  if (lastContent) {
    fitAudienceCanvas(lastContent.slide_dimensions);
    renderSlideInto(canvas, lastContent, false);
  }
});

// Keyboard controls if audience window receives focus
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" || e.key.toLowerCase() === "q") {
    e.preventDefault();
    if (events) events.emit("present-exit", {}).catch(() => {});
  } else if (e.key === "ArrowRight" || e.key === "PageDown" || e.key === " " || e.key === "Enter") {
    if (events) events.emit("present-next", {}).catch(() => {});
  } else if (e.key === "ArrowLeft" || e.key === "PageUp") {
    if (events) events.emit("present-prev", {}).catch(() => {});
  }
});

// Hide cursor after 2.5s of inactivity
let cursorTimer = null;
window.addEventListener("mousemove", () => {
  document.body.classList.add("show-cursor");
  clearTimeout(cursorTimer);
  cursorTimer = setTimeout(() => document.body.classList.remove("show-cursor"), 2500);
});
