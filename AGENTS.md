# Autonomous Agent Directives for OmaDeck (formerly Omashow)

## Project Identity & Branding: OmaDeck
- The application display name and branding is **OmaDeck** (to distinguish from the upstream C++/Qt6 tool `omashow`).
- The repository has integrated upstream design assets, icons, screenshots, and CLI reference specs in `upstream_assets/` (`examples/`, `icons/`, `docs/screenshots/`, and `docs/cli.md`). Consult these for UI and CLI alignment.
- Keep internal Rust crate names as they are to avoid churn, but user-facing UI, titles, exports, and Tauri bundle names must consistently reflect **OmaDeck**.

<!-- antislop:start -->
## antislop
- Antislop applies automatically and silently inline during code authoring.
- **NEVER prompt or ask the user** when antislop applies (neither before starting nor after completion).
- Proceed directly with implementation without stopping for antislop confirmation.
<!-- antislop:end -->

## Loop & Zero-Prompting Autonomy Mode
- **Unattended autonomous execution**: Do NOT stop to ask the user for confirmation, interactive decisions, clarification, or permission during coding sessions.
- **Circuit breaker override**: Do NOT stop or ask for user guidance on build or test failures. Instead, automatically inspect compiler error messages, check `git diff`, refine the implementation, and retry.
- **Strict verification oracle**: Before considering any task complete, you must run and pass:
  1. `cargo test -p omashow-core`
  2. `cargo check --workspace`
  3. Relevant example verification (e.g. `cargo run -p omashow-core --example verify_real`) when modifying PPTX roundtrip logic.
- **Backlog progression**: Always check `TODO.md`. Pick the first unchecked item `[ ]`, implement it completely, verify with the test suite, update `TODO.md` to mark it `[x]`, and make a clean git commit.
- **Continuous loop**: Once a task is committed, immediately continue to the next unchecked task until the active milestone is complete.

## Product Vision & Core Architecture Pillars

### Pillar 1: Exact PPTX Visual Formatting & Rendering Fidelity
- **Typography & Runs**: Render text runs preserving exact font family, size in points, bold, italic, line breaks, and alignment (left/center/right).
- **Geometric Precision**: Convert OpenXML EMUs to slide canvas CSS coordinates proportionally ($914,400\text{ EMUs} = 1\text{ inch}$; standard widescreen $12,192,000 \times 6,858,000\text{ EMUs}$). Keep shape aspect ratios crisp.
- **Color & Style Fidelity**: Preserve text colors, shape fills (solid, gradient), borders, and slide backgrounds.
- **Media & Pictures**: Extract embedded media (`ppt/media/*`) and render pictures within their slide bounding boxes.

### Pillar 2: Dual-Screen Presenter Mode with Multitasking Support
- **Audience Display Window (`audience`)**: Dedicated, borderless/fullscreen window targeting the secondary monitor/projector. Zero chrome, zero toolbars, showing only the active slide.
- **Presenter Console Window (`main`)**: Displayed on the user's primary/laptop screen showing:
  1. Live view of the current slide.
  2. Next-slide preview thumbnail.
  3. Formatted speaker notes extracted from PPTX (`pres.slides[i].notes`).
  4. Elapsed presentation timer & clock.
  5. Slide jump filmstrip.
- **Multitasking Resilience**: The Audience window MUST remain rock-solid, fullscreen, and rendered on the secondary monitor even when the user switches windows or works in another application on the primary monitor.
- **Event Synchronization**: Slide navigation in the console or via hotkeys (`F5`, `Space`, `ArrowRight`, `ArrowLeft`, `B` for blackout) broadcasts instant sync events to the audience window.
- **Single-Monitor & Fullscreen Behavior**: When presenting (especially on single-screen setups like laptops), the main window MUST automatically enter fullscreen mode (`set_fullscreen(true)`) upon entering presentation mode, and restore normal window size (`set_fullscreen(false)`) upon exiting (`Esc` / `exitPresent()`). The user's desktop workspace and wallpaper must NEVER be visible around or through the presentation window.

