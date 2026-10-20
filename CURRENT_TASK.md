# CURRENT TASK: Touch Navigation Gestures (Stretch Milestone)

## Goal
Implement touch navigation gestures for the slide viewer:
- Horizontal touch swipe gestures: swipe left to advance to next slide, swipe right to go to previous slide.
- Tap gesture: double-tap or two-finger tap to toggle speaker notes visibility.
- Prevent conflicting gestures when interacting with stage tools or drawing overlays.

## Steps
1. In `apps/omashow-tauri/src/frontend/main.js`:
   - Add touch event listeners (`touchstart`, `touchmove`, `touchend`) to `#slide-canvas` / `#preview-wrap`.
   - Calculate swipe deltaX / deltaY with a minimum threshold (e.g., 50px).
   - Trigger `nextSlide()` on left swipe and `prevSlide()` on right swipe.
   - Wire double-tap / tap gesture to toggle speaker notes visibility (`#notes-bar`).
2. Test with `cargo test --workspace` and `cargo check -p omashow-tauri`.
3. Update `TODO.md` to mark `- [x] Add touch navigation gestures to slide viewer`.
4. Git commit: `feat(touch): add swipe navigation and tap notes toggle`
