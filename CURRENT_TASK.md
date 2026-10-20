# CURRENT TASK: Animation Engine & Timeline (`ANIMATE`) (Milestone 8)

## Goal
Implement the Animation Engine and Timeline for `ANIMATE` mode:
- Multi-track timeline ruler in seconds (`0.0, 0.5, 1.0, 1.5...`), object duration bars, playhead scrubber, and speed selector (`1x`, `0.5x`, `2x`).
- Build Inspector with Build In/Out timing (`At time`, `On click`, `With/After previous`), duration, and easing curves.
- Morph slide transitions: automatically detect matching shape names/IDs across consecutive slides (using `omashow-core`'s `detect_morph`) and interpolate position, scale, and opacity smoothly.

## Context
- The markup and CSS for `#animate-panel`, `#timeline`, `#tl-playhead`, and `#insp-animate` are already in `apps/omashow-tauri/src/frontend/index.html`.
- Morph detection is implemented in `crates/omashow-core/src/morph.rs` and exposed via Tauri `morph_between`.

## Steps
1. Wire up timeline interactions and playhead animation in `apps/omashow-tauri/src/frontend/main.js`.
2. Connect build inspector inputs (`anim-effect`, `anim-type`, `anim-start`, `anim-duration`, `anim-delay`, `anim-easing`) to active shape animations.
3. Test with `cargo test --workspace` and check `cargo check -p omashow-tauri`.
4. Update `TODO.md` to mark `- [x] Animation Engine & Timeline (\`ANIMATE\`)`.
5. Commit and finish cleanly.
