// Faithful slide rendering, shared by the editor (main.js) and the audience
// window (audience.js). One slide's shapes are drawn into a positioned
// container, scaled from EMU slide coordinates: a filmstrip .slide-thumb,
// the main #slide-stage, or the audience #slide-canvas.

const NON_SOLID_FILLS = new Set(["none", "gradient", "pattern", "image"]);

// Theme engine (sorter mode): when a deck-wide theme is active, every
// solid color is looked up in window.themeRemap (lowercase hex without "#"
// -> replacement CSS color) before painting. null means "original colors".
function themeColor(c) {
  if (!c || !window.themeRemap) return c;
  const key = String(c).replace(/^#/, "").toLowerCase();
  return window.themeRemap[key] || c;
}

function renderSlideInto(container, content, mini = false) {
  container.querySelectorAll(".slide-shape,.slide-pic,.slide-media").forEach((el) => el.remove());
  if (content.background && !NON_SOLID_FILLS.has(content.background)) {
    const bg = themeColor(content.background);
    container.style.backgroundColor = bg;
  } else {
    container.style.backgroundColor = "";
  }
  const dims = content.slide_dimensions;
  const w = container.clientWidth || (mini ? 244 : 960);
  const pxPerEmu = w / dims.width_emu;
  const pxPerInch = pxPerEmu * 914400;
  drawShapes(container, content.shapes, pxPerEmu, pxPerInch, mini);
}

// Embedded media (video/audio) shapes. Video gets a <video> with loop +
// autoplay and a pause/unmute control bar; audio gets a <audio> with the
// native controls. Thumbnails render a static glyph instead of a player.
function drawMediaShape(container, sh, pxPerEmu, mini) {
  if (!sh.bounds || !sh.media) return;
  const wrap = document.createElement("div");
  wrap.className = "slide-media slide-shape";
  wrap.dataset.shapeId = sh.id;
  wrap.title = sh.name || (sh.media.media_type === "video" ? "Video" : "Audio");
  place(wrap, sh.bounds, pxPerEmu);

  if (mini) {
    const glyph = document.createElement("div");
    glyph.className = "media-glyph";
    glyph.textContent = sh.media.media_type === "video" ? "▶" : "♪";
    wrap.appendChild(glyph);
    container.appendChild(wrap);
    return;
  }

  const el = document.createElement(sh.media.media_type === "video" ? "video" : "audio");
  el.src = sh.media.data_uri;
  el.loop = true;
  el.autoplay = true;
  el.playsInline = true;
  el.preload = "auto";
  // Browsers block unmuted autoplay; start muted and let the user unmute.
  el.muted = true;
  if (sh.media.media_type === "audio") el.controls = true;
  el.addEventListener("canplay", () => { el.play().catch(() => {}); }, { once: true });
  wrap.appendChild(el);

  const bar = document.createElement("div");
  bar.className = "media-ctl";
  const pauseBtn = document.createElement("button");
  pauseBtn.type = "button";
  pauseBtn.textContent = "❚❚";
  pauseBtn.title = "Pause";
  pauseBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    e.currentTarget.blur();
    if (el.paused) {
      el.play().catch(() => {});
      pauseBtn.textContent = "❚❚";
      pauseBtn.title = "Pause";
    } else {
      el.pause();
      pauseBtn.textContent = "▶";
      pauseBtn.title = "Play";
    }
  });
  const muteBtn = document.createElement("button");
  muteBtn.type = "button";
  muteBtn.textContent = "🔇";
  muteBtn.title = "Unmute";
  muteBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    e.currentTarget.blur();
    el.muted = !el.muted;
    muteBtn.textContent = el.muted ? "🔇" : "🔊";
    muteBtn.title = el.muted ? "Unmute" : "Mute";
    if (!el.muted) el.play().catch(() => {});
  });
  bar.append(pauseBtn, muteBtn);
  wrap.appendChild(bar);
  container.appendChild(wrap);
}

function drawShapes(container, shapes, pxPerEmu, pxPerInch, mini) {
  for (const sh of shapes) {
    if (sh.children) { drawShapes(container, sh.children, pxPerEmu, pxPerInch, mini); continue; }
    if (sh.kind === "picture") {
      if (!sh.bounds) continue;
      const pic = document.createElement("div");
      pic.className = "slide-pic";
      pic.dataset.shapeId = sh.id;
      place(pic, sh.bounds, pxPerEmu);
      if (sh.pic && sh.pic.data_uri) {
        const img = document.createElement("img");
        img.src = sh.pic.data_uri;
        img.alt = sh.name || "";
        pic.appendChild(img);
      }
      container.appendChild(pic);
      continue;
    }
    if (sh.kind === "table") {
      if (!sh.bounds || !sh.table) continue;
      const tblWrap = document.createElement("div");
      tblWrap.className = "slide-table-wrap slide-shape";
      tblWrap.dataset.shapeId = sh.id;
      place(tblWrap, sh.bounds, pxPerEmu);

      const tableEl = document.createElement("table");
      tableEl.className = "slide-table";
      tableEl.style.width = "100%";
      tableEl.style.height = "100%";
      tableEl.style.borderCollapse = "collapse";
      tableEl.style.tableLayout = "fixed";

      // Colgroup
      if (sh.table.column_widths_emu && sh.table.column_widths_emu.length) {
        const colgroup = document.createElement("colgroup");
        for (const wEmu of sh.table.column_widths_emu) {
          const col = document.createElement("col");
          col.style.width = (wEmu * pxPerEmu) + "px";
          colgroup.appendChild(col);
        }
        tableEl.appendChild(colgroup);
      }

      const tbody = document.createElement("tbody");
      sh.table.rows.forEach((row, rowIdx) => {
        const tr = document.createElement("tr");
        if (sh.table.row_heights_emu && sh.table.row_heights_emu[rowIdx]) {
          tr.style.height = (sh.table.row_heights_emu[rowIdx] * pxPerEmu) + "px";
        }
        row.forEach((cell) => {
          const td = document.createElement("td");
          if (cell.col_span > 1) td.colSpan = cell.col_span;
          if (cell.row_span > 1) td.rowSpan = cell.row_span;

          // Padding and typography
          td.style.padding = (mini ? "1px 2px" : "4px 6px");
          td.style.verticalAlign = "top";
          td.style.boxSizing = "border-box";

          // Border formatting
          const defaultBorder = mini ? "0.5px solid rgba(255,255,255,0.15)" : "1px solid rgba(255,255,255,0.2)";
          td.style.borderTop = cell.border_top && cell.border_top.color
            ? `${Math.max(1, (cell.border_top.width_emu || 9525) * pxPerEmu)}px solid ${themeColor(cell.border_top.color)}`
            : defaultBorder;
          td.style.borderBottom = cell.border_bottom && cell.border_bottom.color
            ? `${Math.max(1, (cell.border_bottom.width_emu || 9525) * pxPerEmu)}px solid ${themeColor(cell.border_bottom.color)}`
            : defaultBorder;
          td.style.borderLeft = cell.border_left && cell.border_left.color
            ? `${Math.max(1, (cell.border_left.width_emu || 9525) * pxPerEmu)}px solid ${themeColor(cell.border_left.color)}`
            : defaultBorder;
          td.style.borderRight = cell.border_right && cell.border_right.color
            ? `${Math.max(1, (cell.border_right.width_emu || 9525) * pxPerEmu)}px solid ${themeColor(cell.border_right.color)}`
            : defaultBorder;

          // Fill / background
          const cellFill = themeColor(cell.fill);
          if (cellFill && !NON_SOLID_FILLS.has(cellFill)) {
            td.style.background = cellFill;
          } else if (sh.table.first_row && rowIdx === 0) {
            td.style.background = "rgba(99, 102, 241, 0.25)";
            td.style.fontWeight = "600";
          } else if (sh.table.band_rows && rowIdx % 2 === 1) {
            td.style.background = "rgba(255, 255, 255, 0.04)";
          }

          if (cell.runs && cell.runs.length) {
            td.style.fontSize = ptToPx(14, pxPerInch, mini);
            const aligned = cell.runs.find((r) => r.alignment);
            if (aligned) td.style.textAlign = aligned.alignment;
            appendRuns(td, cell.runs, pxPerInch, mini);
          } else if (cell.text) {
            td.style.fontSize = ptToPx(14, pxPerInch, mini);
            td.textContent = cell.text;
          }

          tr.appendChild(td);
        });
        tbody.appendChild(tr);
      });
      tableEl.appendChild(tbody);
      tblWrap.appendChild(tableEl);
      container.appendChild(tblWrap);
      continue;
    }
    if (sh.kind === "connector") {
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.className = "slide-connector";
      svg.dataset.shapeId = sh.id;
      svg.style.position = "absolute";
      svg.style.left = "0";
      svg.style.top = "0";
      svg.style.width = "100%";
      svg.style.height = "100%";
      svg.style.pointerEvents = "none";
      svg.style.overflow = "visible";

      // If connected to shapes, resolve anchor points; otherwise use bounds
      let x1 = 0, y1 = 0, x2 = 0, y2 = 0;
      if (sh.bounds) {
        x1 = sh.bounds.x_emu * pxPerEmu;
        y1 = sh.bounds.y_emu * pxPerEmu;
        x2 = (sh.bounds.x_emu + sh.bounds.width_emu) * pxPerEmu;
        y2 = (sh.bounds.y_emu + sh.bounds.height_emu) * pxPerEmu;
      }
      if (sh.connector) {
        if (sh.connector.start_connection) {
          const src = shapes.find(s => s.id === sh.connector.start_connection.shape_id);
          if (src && src.bounds) {
            x1 = (src.bounds.x_emu + src.bounds.width_emu / 2) * pxPerEmu;
            y1 = (src.bounds.y_emu + src.bounds.height_emu / 2) * pxPerEmu;
          }
        }
        if (sh.connector.end_connection) {
          const dst = shapes.find(s => s.id === sh.connector.end_connection.shape_id);
          if (dst && dst.bounds) {
            x2 = (dst.bounds.x_emu + dst.bounds.width_emu / 2) * pxPerEmu;
            y2 = (dst.bounds.y_emu + dst.bounds.height_emu / 2) * pxPerEmu;
          }
        }
      }

      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      line.setAttribute("x1", x1);
      line.setAttribute("y1", y1);
      line.setAttribute("x2", x2);
      line.setAttribute("y2", y2);
      const strokeColor = themeColor(sh.line && sh.line.color) || "#94a3b8";
      const strokeWidth = sh.line && sh.line.width_emu ? Math.max(1, (sh.line.width_emu / 914400) * pxPerInch) : 2;
      line.setAttribute("stroke", strokeColor);
      line.setAttribute("stroke-width", strokeWidth);
      svg.appendChild(line);
      container.appendChild(svg);
      continue;
    }
    if (sh.kind === "media") {
      drawMediaShape(container, sh, pxPerEmu, mini);
      continue;
    }
    if (sh.kind !== "autoshape" || !sh.bounds) continue;
    const el = document.createElement("div");
    el.className = "slide-shape";
    el.dataset.shapeId = sh.id;
    place(el, sh.bounds, pxPerEmu);
    const fill = themeColor(sh.fill);
    if (fill && !NON_SOLID_FILLS.has(fill)) el.style.background = fill;
    const lineColor = themeColor(sh.line && sh.line.color);
    if (sh.line && lineColor && sh.line.width_emu) {
      const bw = Math.max(mini ? 1 : 0.5, (sh.line.width_emu / 914400) * pxPerInch);
      el.style.border = bw + "px solid " + lineColor;
    }
    if (sh.runs.length) {
      const basePt = sh.placeholder === "title" || sh.placeholder === "ctrTitle" ? 28 : 18;
      el.style.fontSize = ptToPx(basePt, pxPerInch, mini);
      const aligned = sh.runs.find((r) => r.alignment);
      if (aligned) el.style.textAlign = aligned.alignment;
      appendRuns(el, sh.runs, pxPerInch, mini);
    }
    container.appendChild(el);
  }
}

function ptToPx(pt, pxPerInch, mini) {
  const px = (pt / 72) * pxPerInch;
  return (mini ? Math.max(4, Math.min(16, px)) : px) + "px";
}

function place(el, b, pxPerEmu) {
  el.style.left = b.x_emu * pxPerEmu + "px";
  el.style.top = b.y_emu * pxPerEmu + "px";
  el.style.width = b.width_emu * pxPerEmu + "px";
  el.style.height = b.height_emu * pxPerEmu + "px";
}

function appendRuns(el, runs, pxPerInch, mini = false) {
  let para = 0;
  for (const r of runs) {
    if (r.paragraph !== para) {
      el.appendChild(document.createElement("br"));
      para = r.paragraph;
    }
    if (r.text === "\n") {
      el.appendChild(document.createElement("br"));
      continue;
    }
    const s = document.createElement("span");
    s.textContent = r.text;
    if (r.bold) s.style.fontWeight = "700";
    if (r.italic) s.style.fontStyle = "italic";
    const rc = themeColor(r.color);
    if (rc) s.style.color = rc;
    // Explicit sans fallback: an unknown family (e.g. Calibri on Linux) would
    // otherwise degrade to the browser's default serif.
    if (r.font_family) s.style.fontFamily = r.font_family + ", system-ui, sans-serif";
    if (r.font_size_pt) s.style.fontSize = ptToPx(r.font_size_pt, pxPerInch, mini);
    el.appendChild(s);
  }
}
