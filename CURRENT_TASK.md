# CURRENT TASK: Floating Stage Tools & Filmstrip Polish (`EDIT`) (Milestone 8)

## Goal
Implement the floating stage tools pill on the editor slide canvas (`Pen`, `Freehand`, `Nodes` vector pen, and `Snap to guides` toggle) and add section headers with slide counts and action badges to the filmstrip.

## Context
- The HTML/CSS structure for `#stage-tools` and `.fs-section` is already staged in `apps/omashow-tauri/src/frontend/index.html`.
- `#stage-ink` SVG overlay is already placed on `#slide-canvas`.

## Steps
1. In `apps/omashow-tauri/src/frontend/main.js`:
   - Wire up the stage tool pill buttons (`#pen-mode-seg button`, `#snap-toggle`, `#stage-ink-undo`, `#stage-ink-clear`):
     - `pen`: fine opaque freehand vector strokes.
     - `freehand`: translucent highlighter strokes.
     - `nodes`: click to place connected vector nodes, drag nodes to reshape.
     - `snap`: snap coordinates to slide center and third guides.
   - Render filmstrip section dividers with slide count badges (`.fs-section`, `.fs-section-name`, `.fs-section-range`) and slide action badges (✨ AI / ✏️ Edit title).
2. Verification:
   - Check `cargo test -p omashow-core` and run `cargo check -p omashow-tauri`.
   - Update `TODO.md` to mark `- [x] Floating Stage Tools & Filmstrip Polish (\`EDIT\`)`.
   - Commit: `feat(editor): add floating stage tool pill and filmstrip section headers`
