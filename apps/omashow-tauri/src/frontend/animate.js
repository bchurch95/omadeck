// animate.js - Animation Engine & Timeline for OmaDeck (Milestone 8)
// Manages object builds (in/out), multi-track timeline, playhead scrubbing, and morph transitions.

(function() {
  let animTracks = []; // { id, name, type, effect, start, duration, delay, easing }
  let playheadPos = 0; // seconds
  let isPlaying = false;
  let playTimer = null;
  let playSpeed = 1.0;
  let selectedAnimIdx = -1;

  function initAnimate() {
    const playBtn = document.getElementById("anim-play");
    const speedSeg = document.getElementById("anim-speed");
    const addInBtn = document.getElementById("anim-add-in");
    const addOutBtn = document.getElementById("anim-add-out");
    const removeBtn = document.getElementById("anim-remove");
    const targetSelect = document.getElementById("anim-target");
    const morphChk = document.getElementById("anim-morph");

    if (speedSeg) {
      speedSeg.querySelectorAll("button").forEach(b => {
        b.onclick = () => {
          speedSeg.querySelectorAll("button").forEach(x => x.classList.remove("on"));
          b.classList.add("on");
          playSpeed = parseFloat(b.dataset.speed || "1");
        };
      });
    }

    if (playBtn) {
      playBtn.onclick = togglePlay;
    }

    if (addInBtn) {
      addInBtn.onclick = () => addBuild("in");
    }

    if (addOutBtn) {
      addOutBtn.onclick = () => addBuild("out");
    }

    if (removeBtn) {
      removeBtn.onclick = removeSelectedBuild;
    }

    const delPropBtn = document.getElementById("anim-prop-del");
    if (delPropBtn) {
      delPropBtn.onclick = removeSelectedBuild;
    }

    // Bind property inspector controls
    ["anim-effect", "anim-type", "anim-start", "anim-duration", "anim-delay", "anim-easing"].forEach(id => {
      const el = document.getElementById(id);
      if (el) {
        el.onchange = () => updateSelectedProperties();
      }
    });

    renderRuler();
    renderTimeline();
  }

  function renderRuler() {
    const ruler = document.getElementById("timeline-ruler");
    if (!ruler) return;
    ruler.innerHTML = "";
    // 10 seconds ruler, marks every 0.5s
    for (let s = 0; s <= 10; s += 0.5) {
      const mark = document.createElement("div");
      mark.style.position = "absolute";
      mark.style.left = (s * 80) + "px";
      mark.style.fontSize = "10px";
      mark.style.color = "var(--dim)";
      mark.textContent = s.toFixed(1) + "s";
      ruler.appendChild(mark);
    }
  }

  function addBuild(type) {
    const targetSelect = document.getElementById("anim-target");
    const shapeId = targetSelect ? parseInt(targetSelect.value || "0") : 0;
    const shapeName = targetSelect && targetSelect.selectedOptions[0] ? targetSelect.selectedOptions[0].textContent : `Shape ${shapeId}`;

    const build = {
      shapeId: shapeId,
      name: shapeName,
      type: type, // 'in' or 'out'
      effect: "fade",
      start: "at time",
      duration: 1.0,
      delay: animTracks.length * 0.5,
      easing: "ease-in-out"
    };

    animTracks.push(build);
    selectedAnimIdx = animTracks.length - 1;
    renderTimeline();
    syncInspector();
  }

  function removeSelectedBuild() {
    if (selectedAnimIdx >= 0 && selectedAnimIdx < animTracks.length) {
      animTracks.splice(selectedAnimIdx, 1);
      selectedAnimIdx = Math.min(selectedAnimIdx, animTracks.length - 1);
      renderTimeline();
      syncInspector();
    }
  }

  function updateSelectedProperties() {
    if (selectedAnimIdx < 0 || selectedAnimIdx >= animTracks.length) return;
    const b = animTracks[selectedAnimIdx];
    const getVal = id => { const el = document.getElementById(id); return el ? el.value : ""; };
    b.effect = getVal("anim-effect");
    b.type = getVal("anim-type");
    b.start = getVal("anim-start");
    b.duration = parseFloat(getVal("anim-duration") || "1.0");
    b.delay = parseFloat(getVal("anim-delay") || "0");
    b.easing = getVal("anim-easing");
    renderTimeline();
  }

  function syncInspector() {
    const emptyMsg = document.getElementById("anim-prop-empty");
    const editForm = document.getElementById("anim-prop-edit");
    const removeBtn = document.getElementById("anim-remove");

    if (selectedAnimIdx < 0 || selectedAnimIdx >= animTracks.length) {
      if (emptyMsg) emptyMsg.style.display = "block";
      if (editForm) editForm.style.display = "none";
      if (removeBtn) removeBtn.disabled = true;
      return;
    }

    if (emptyMsg) emptyMsg.style.display = "none";
    if (editForm) editForm.style.display = "block";
    if (removeBtn) removeBtn.disabled = false;

    const b = animTracks[selectedAnimIdx];
    const setVal = (id, v) => { const el = document.getElementById(id); if (el) el.value = v; };
    setVal("anim-effect", b.effect);
    setVal("anim-type", b.type);
    setVal("anim-start", b.start);
    setVal("anim-duration", b.duration);
    setVal("anim-delay", b.delay);
    setVal("anim-easing", b.easing);
  }

  function renderTimeline() {
    const lanes = document.getElementById("timeline-lanes");
    const empty = document.getElementById("timeline-empty");
    const countBadge = document.getElementById("anim-count");

    if (!lanes) return;
    lanes.innerHTML = "";

    if (animTracks.length === 0) {
      if (empty) empty.style.display = "block";
      if (countBadge) countBadge.textContent = "0 builds";
      return;
    }

    if (empty) empty.style.display = "none";
    if (countBadge) countBadge.textContent = `${animTracks.length} build${animTracks.length > 1 ? "s" : ""}`;

    animTracks.forEach((b, idx) => {
      const lane = document.createElement("div");
      lane.className = "timeline-lane";
      lane.style.display = "flex";
      lane.style.alignItems = "center";
      lane.style.height = "28px";
      lane.style.position = "relative";
      lane.style.borderBottom = "1px solid rgba(255,255,255,0.05)";

      const bar = document.createElement("div");
      bar.className = "timeline-bar";
      const startPx = (b.delay || 0) * 80;
      const widthPx = Math.max(30, (b.duration || 1) * 80);
      bar.style.position = "absolute";
      bar.style.left = startPx + "px";
      bar.style.width = widthPx + "px";
      bar.style.height = "20px";
      bar.style.borderRadius = "4px";
      bar.style.background = b.type === "in" ? "var(--accent)" : "#e06c75";
      bar.style.color = "#fff";
      bar.style.fontSize = "11px";
      bar.style.display = "flex";
      bar.style.alignItems = "center";
      bar.style.padding = "0 6px";
      bar.style.cursor = "pointer";
      bar.style.boxSizing = "border-box";
      bar.style.whiteSpace = "nowrap";
      bar.style.overflow = "hidden";
      bar.style.textOverflow = "ellipsis";
      bar.textContent = `${b.name} (${b.type}: ${b.effect})`;

      if (idx === selectedAnimIdx) {
        bar.style.outline = "2px solid #fff";
      }

      bar.onclick = (e) => {
        e.stopPropagation();
        selectedAnimIdx = idx;
        renderTimeline();
        syncInspector();
      };

      lane.appendChild(bar);
      lanes.appendChild(lane);
    });
  }

  function togglePlay() {
    isPlaying = !isPlaying;
    const playBtn = document.getElementById("anim-play");
    const playhead = document.getElementById("tl-playhead");

    if (isPlaying) {
      if (playBtn) playBtn.innerHTML = '<span>Pause</span>';
      if (playhead) playhead.style.display = "block";
      playheadPos = 0;
      const interval = 30; // 30ms
      playTimer = setInterval(() => {
        playheadPos += (interval / 1000) * playSpeed;
        if (playhead) {
          playhead.style.left = (playheadPos * 80) + "px";
        }
        if (playheadPos > 10) {
          stopPlay();
        }
      }, interval);
    } else {
      stopPlay();
    }
  }

  function stopPlay() {
    isPlaying = false;
    if (playTimer) clearInterval(playTimer);
    playTimer = null;
    const playBtn = document.getElementById("anim-play");
    if (playBtn) {
      playBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M8 5v14l11-7z"/></svg><span>Play</span>';
    }
    const playhead = document.getElementById("tl-playhead");
    if (playhead) playhead.style.display = "none";
  }

  // Populate shape targets whenever slide changes
  window.updateAnimateTargets = function(shapes) {
    const sel = document.getElementById("anim-target");
    if (!sel) return;
    sel.innerHTML = "";
    (shapes || []).forEach(s => {
      const opt = document.createElement("option");
      opt.value = s.id;
      opt.textContent = s.name || `Shape ${s.id}`;
      sel.appendChild(opt);
    });
  };

  window.addEventListener("DOMContentLoaded", initAnimate);
})();
