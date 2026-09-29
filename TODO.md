# OmaDeck Development Backlog

## Milestone 1: Robust Lossless PPTX Roundtripping (Complete)
- [x] Run and verify `crates/omashow-core/examples/verify_real.rs` against real `python-pptx` decks (`/tmp/omashow-rt/real.pptx`).
- [x] Ensure non-writer-modified OPC package parts (media, audio, video, embedded fonts, unknown XML relations) are preserved.
- [x] Add integration test in `crates/omashow-core/tests/roundtrip.rs` asserting no-op save is byte-identical to source.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings` and fix any compiler warnings or lint issues.

## Milestone 2: Headless Slide Extraction & Inspection API (Complete)
- [x] Expose slide metadata APIs in `omashow-core` (`slide_count()`, `slide_dimensions()`, `get_slide_shapes()`).
- [x] Implement CLI inspect command: `cargo run -p omashow-cli -- inspect <path.pptx>` outputting structured JSON.
- [x] Add test cases verifying correct text run and bounding box extraction across multi-shape slides.

## Milestone 3: High-Fidelity Slide Canvas & Viewer (Complete)
- [x] Update Tauri backend commands in `apps/omashow-tauri/src-tauri/src/main.rs`:
  - `open_presentation(path)` returning metadata, slide list, and speaker notes.
  - `get_slide_content(slide_idx)` returning shapes, text runs, colors, bounding boxes, and dimensions.
  - `save_presentation(path)`.
- [x] Implement thumbnail filmstrip sidebar in the Tauri frontend with slide numbers and active indicator.
- [x] Build high-fidelity slide canvas preserving typography (fonts, sizes in pt, bold/italic, alignment) and proportional EMU bounding boxes.
- [x] Run visual audit with `python3 visual_critic.py` and refine CSS/layout aesthetics until Qwen Vision scores the interface 8/10 or higher.

## Milestone 4: Dual-Screen Presenter Mode & Multitasking (Complete)
- [x] Implement Tauri multi-window commands (`open_audience_window(monitor_id)`, `close_audience_window`):
  - Detect secondary monitor/projector via Tauri display API and position audience window fullscreen.
  - Keep audience window visible, borderless, and non-minimizing when primary window loses OS focus during multitasking.
- [x] Build Presenter Console on primary screen:
  - Live active slide view + Next-slide preview thumbnail.
  - Formatted speaker notes extracted from PPTX (`pres.slides[i].notes`).
  - Elapsed presentation timer and current clock.
- [x] Real-time event synchronization between Presenter Console and Audience Window (`slide-changed`, `blackout-toggle`).
- [x] Keyboard navigation: `F5` (launch dual-screen presentation), `Space`/`ArrowRight` (advance), `ArrowLeft` (previous), `B` (blackout audience screen), `Escape` (exit).

## Milestone 5: Slide Editing, Undo History & Filmstrip Reordering (Complete)
- [x] Add `UndoStack` command pattern for all slide and text mutations.
- [x] Add `update_text_run(slide_idx, shape_id, new_text)` in `omashow-core`.
- [x] Add `add_blank_slide(index)` and `delete_slide(index)` with filmstrip drag/reorder controls.
- [x] Connect Tauri frontend text editing events to core mutation commands.

## Milestone 6: Image Extraction & Multi-Vendor Corpus Testing (Complete)
- [x] Support `<p:pic>` shape extraction and map embedded media relationships (`ppt/media/*`).
- [x] Render embedded slide pictures inside the Tauri slide canvas. (Editor canvas, filmstrip thumbnails, presenter console and audience window; `ShapeInfo.pic` carries format + data URI, e2e-verified under Xvfb.)
- [x] Add multi-vendor test suite in `crates/omashow-core/tests/corpus.rs` verifying decks from Google Slides, M365, and Keynote. (8 Apache-2.0 Apache POI fixtures: M365 PowerPoint 2007–2016 Windows + macOS, LibreOffice 5–25; two SmartArt/OLE fixtures pinned as `KNOWN_REJECTIONS` with a stable non-panicking error contract.)
- [x] Add negative tests ensuring corrupt PPTX files fail gracefully with typed `Result` errors. (6 cases in `tests/corrupt.rs`: empty file, garbage bytes, truncated zip, missing `[Content_Types].xml`, invalid content-types XML, corrupted slide XML.)

## Milestone 7: Presenter Stage Tools & Export Engine (PDF & Web)
- [x] Virtual Laser Pointer: Holding `Ctrl` or selecting pointer tool projects a glowing laser dot onto the audience screen synchronized with cursor movement. (Ctrl-hold + console toggle button; dot follows cursor 1:1 in slide space, e2e-verified on Xvfb.)
- [x] Live Slide Drawing & Highlighter: Transparent SVG overlay for in-show pen/marker annotation over active slides (per-slide ink storage, undo/clear, `D`/`M`/`U`/`C` shortcuts, live sync to audience). E2e-pixel-verified on Xvfb. Also fixed placeholder geometry inheritance (slide→layout→master) so python-pptx decks no longer render blank.
- [x] Slide Grid Navigator: Hitting `G` during presentation displays a full-screen thumbnail matrix to jump directly to any slide. (G toggles, Esc closes then exits, footer button; thumbnails render live slide content with number badges + title labels, current slide highlighted, click jumps and syncs to audience. E2e-pixel-verified on Xvfb.)
- [x] Vector PDF Export: Implement `omashow-cli export-pdf <deck.pptx> <out.pdf>` and a Tauri "Export to PDF" dialog with 1:1 vector precision. (printpdf-based vector writer in `crates/omashow-core/src/export_pdf.rs`: per-slide pages sized from `sldSz`, EMU-accurate shape fills/outlines, font-mapped text with paragraph line breaks, embedded pictures via image decoding; `omashow export-pdf` CLI subcommand + Tauri `export_pdf` command wired to a Save dialog. 4 integration + 5 unit tests; CLI and in-app save-dialog e2e pixel-verified on Xvfb.)
- [x] Standalone HTML5 Bundle Export: Export presentation as an offline, single-file HTML presentation viewable in any browser. (`crates/omashow-core/src/export_html.rs`: one `<section>` per slide at 96 CSS px/inch, absolutely positioned shape/text/picture nodes reusing the inspect data-URIs, per-run font/size/weight/color; embedded script handles arrow/space/Home/End navigation, fit-to-window scaling, counter, `F` fullscreen and `N` notes. `omashow export-html` CLI subcommand + Tauri `export_html` command + Export HTML toolbar button. 5 integration tests; headless-Chromium render and in-app save-dialog e2e verified.)

## Milestone 8: Official OmaShow UI Suite & Visual Polish (Matching Video Demo Specs)
*Reference: `design_spec/OMASHOW_UI_SPEC.md` and screenshots in `design_spec/screenshots/`*
- [x] Top Mode Bar Switcher: Implement the 7 primary application modes (`EDIT`, `DESIGN`, `ANIMATE`, `REVIEW`, `PRESENT`, `EXPORT`, `SORTER`) with active tab states and keyboard shortcuts.
- [x] Presenter Console 3-Pane Layout: (3-pane `body.presenting` layout: left CURRENT stage with build stepper + prev/next, right NEXT thumbnail + zoomable speaker notes + PRESENTATION CONTROLS (elapsed HH:MM:SS with pause/restart, target countdown, Black/White/Freeze shutters), bottom SLIDE NAVIGATOR filmstrip; presenter top bar with Live badge, monitor select, Swap Displays, End Show, wall clock; status bar with `Slide X of Y · Build X of Y` + audience monitor label. E2e-pixel-verified on Xvfb: nav sync, black/white/freeze shutters, pause/restart, navigator jump, end-show restore.)
  - Left pane: `CURRENT` slide view with build stepper dots (`Build X of Y — Ready · Next to continue`), previous/next controls.
  - Right pane: `NEXT` thumbnail preview, `SPEAKER NOTES` with font zoom controls (`A- A A+`), and `PRESENTATION CONTROLS` (dual timers: elapsed with pause/restart + target countdown, and `Black`, `White`, `Freeze` screen shutters).
  - Bottom pane: persistent horizontal `SLIDE NAVIGATOR` filmstrip bar.
- [x] Full-Screen Slide Sorter Mode (`SORTER`): (full-screen multi-column thumbnail grid; session-local sections with collapsible/renameable/dividable headers; per-card ◀/▶ moves + HTML5 drag-and-drop batch reordering via `reorder_slides` (single undo step, permutation-validated); global Theme Engine with Original/green/blue/sunset/mono swatches — live recolor through `window.themeRemap` in the shared renderer, and `apply_theme` persisting a case-insensitive, idempotent `srgbClr val="…"` remap across all XML parts on save. E2e-pixel-verified on Xvfb: grid render, section add/collapse/expand, move round-trip, live theme recolor, apply-to-deck XML remap, Done → edit restore with correct stage fit.)
  - Multi-column thumbnail grid with section headers and collapsible dividers (e.g. `OPENING (1-3)`, `FILE NAMING (4-7)`).
  - Drag-and-drop batch slide reordering across sections.
  - Global Theme Engine: instant live recoloring of all slides across the entire deck via design tokens (as shown in `04_sorter_green_theme.png` → `05_sorter_blue_theme.png`).
- [ ] Floating Stage Tools & Filmstrip Polish (`EDIT`):
  - Floating stage tool pill with `Pen`, `Freehand`, `Nodes` vector pen, and `Snap to guides` toggle button.
  - Filmstrip section headers with slide counts and action badges (✨ AI / ✏️ Edit title).
- [ ] Animation Engine & Timeline (`ANIMATE`):
  - Multi-track timeline ruler in seconds (`0.0, 0.5, 1.0, 1.5...`), object duration bars, playhead scrubber, and speed selector (`1x`, `0.5x`, `2x`).
  - Build Inspector with Build In/Out timing (`At time`, `On click`, `With/After previous`), duration, and easing curves.
  - Morph slide transitions: automatically detect matching shape names/IDs across consecutive slides and interpolate position, scale, and opacity smoothly.

### 🔍 Architect & Critic Feedback (Commit 095fe3c - Score: 7/10)
- [ ] [Critic] Replace .unwrap()/.expect() calls in inspect.rs media-relationship resolution with typed Result<_, Error::MediaRelationMissing> returns and add a unit test for a <p:media> node with a dangling rId.
- [ ] [Critic] Restore the Z-Order task spec by moving it to a separate file (e.g., TASKS/z-order.md) instead of overwriting CURRENT_TASK.md, preserving the design notes for Milestone 10.
- [ ] [Critic] Add a try/catch or .catch() guard around media element creation in slide_render.js so a corrupt or missing media blob does not abort the entire slide render loop.
- [ ] [Critic] In audience.js, add a cleanup hook on slide-advance that calls .pause() and .remove() on all active <audio>/<video> elements to prevent orphaned DOM nodes and audio bleed-through.
- [ ] [Critic] Override default browser <video> controls with custom dark-slate-styled controls (play/pause, progress, mute) in both index.html and audience.html to maintain native desktop visual consistency.
- [ ] [Critic] Add a visual regression snapshot test (or manual checklist item) verifying that media shape thumbnails in the editor display a recognizable media-type icon overlay distinct from picture shapes.
- [ ] [Critic] Verify in inspect.rs that unknown child elements and namespaces within <p:media>/<p:audio> are serialized back verbatim during OOXML roundtrip; add a roundtrip test with a deliberately unknown namespace child.

## Milestone 9: Rich Media, Audio/Video & Tables
- [x] Embedded Audio & Video Playback: Play slide media parts (`ppt/media/*.mp4`, `.wav`) with auto-play on slide entry, looping, and pause controls. (`inspect.rs` `MediaInfo { media_type, data_uri, size_bytes }` on `ShapeInfo.media`; shared `drawMediaShape()` in slide_render.js renders a positioned, looping, muted-autoplay `<video>` with pause/unmute control bar or a native-controls `<audio>`, with ▶/♪ glyph thumbnails in filmstrip/sorter/next-preview. Wired into editor canvas, presenter console, and audience window.)
- [x] Table Support (`a:tbl`): Extract, render, and format OpenXML tables with column widths, borders, cell margins, and background fills.
- [x] Smart Magnetic Connectors: Lines and arrows that dynamically anchor between shape boundary points and adjust when shapes move.

## Milestone 10: Canvas Backgrounds & Layer Management
- [x] Slide Background Engine (`p:bg`): Support custom slide background fills (solid hex colors, linear/radial gradients, and background picture fills `p:bgRef` / `blipFill`) with live canvas preview and an "Apply to All Slides" button.
- [x] Z-Order Controls & Stacking: Implement Bring to Front, Send to Back, Bring Forward, and Send Backward actions with keyboard shortcuts (`Cmd/Ctrl + Shift + ] / [`), reordering elements in the DOM and preserving shape order in OOXML `p:spTree`.
- [x] Sidebar Layers Panel: Collapsible layers tree showing all shapes, text boxes, and media on the active slide; support click-to-select, visibility toggle (hide/show eye icon), and layer reordering. (Left-sidebar panel above the filmstrip: z-ordered shape list with kind badges, click-to-select synced with stage highlight, eye-icon visibility toggle persisted per slide and applied to thumbnails, ▲/▼ reorder via `reorder_shape_z_order`, collapse/expand. Lock state, rename, and drag-and-drop remain open.)

## Stretch Milestone: iPadOS & AirPlay External Display Support
- [ ] Add touch navigation gestures to slide viewer (swipe left/right to advance, tap to toggle notes).
- [ ] Configure Tauri v2 iOS/iPadOS mobile target (`cargo tauri ios`).
- [ ] Implement secondary display routing for AirPlay / external screens via `UIScreen` / `UIWindowScene` (dedicated audience window, avoiding simple mirroring).
- [ ] Support Split View / Slide Over multitasking with live AirPlay slide presentation.
- [ ] Background AirPlay video stream mode allowing complete app minimization while keeping audience slides active.

## Stretch: office-toolkit reader gaps & vendor coverage (tracked, not blocking)
- [ ] SmartArt diagrams: office-toolkit mis-parses `dgm:relIds` (diagram parts) as chart relationships and rejects the package. Fix upstream (or vendor + patch) so SmartArt slides open; then promote `m365_smartart.bin` from `KNOWN_REJECTIONS` to `FIXTURES`.
- [ ] Legacy OLE objects / `mc:AlternateContent`: same rejection path (see `m365_bug64693.bin`). Model or gracefully skip `p:oleObj` so the rest of the slide still renders.
- [ ] Genuine Google Slides and Keynote export fixtures: requires licensed/first-party sample decks (no public corpus found); add once available.

## Packaging & Platform Distribution
- [ ] Fix Linux AppImage WebKitGTK crash on Wayland/Mesa: Update runner from ubuntu-22.04 to ubuntu-24.04 and exclude bundled libwayland-client from AppImage (prevents `undefined symbol: wl_fixes_interface` and surfaceless EGL abort on modern hosts like Arch/Omarchy; see `DEBUG_APPIMAGE_CRASH.md`).
- [ ] Single-Monitor Fullscreen Presentation & Solid Opacity:
  - In `startPresentation()` / `enterPresent()`: automatically trigger `set_fullscreen(true)` on the main window when entering presentation mode, and restore `set_fullscreen(false)` on `exitPresent()` / `Esc`, completely covering the desktop so wallpaper and panel gaps are never visible during a presentation.
  - On single-monitor setups, prevent floating preview window clutter over the active presenter console.
  - Ensure window opacity is 100% solid with `#000000`/`#0f1117` backing so window managers (e.g. Hyprland in Omarchy) do not bleed desktop wallpapers through.
