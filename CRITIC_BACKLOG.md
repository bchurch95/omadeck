### 🔍 Architect & Critic Feedback (Commit 28ca6f1 - Score: 7/10)
- [ ] [Critic] Split Milestone 10 into 10a (background engine + p:bg roundtrip), 10b (z-order + spTree reordering), and 10c (layers panel UI) to enable independent PRs and CI gates.
- [ ] [Critic] Add a roundtrip test fixture (a .pptx with p:bgRef, blipFill, and a 200-shape spTree) to the Rust test suite before implementation begins.
- [ ] [Critic] Specify slide-master background inheritance rules in the Milestone 10 doc: define precedence between p:bg on slide vs. layout vs. master and how 'Apply to All Slides' handles inherited backgrounds.
- [ ] [Critic] Create a Figma or ASCII wireframe for the Layers Panel showing eye-icon, lock-icon, and drag-handle placement to ensure visual consistency with the existing dark-slate sidebar.
- [ ] [Critic] Define a performance target for the Layers Panel (e.g., <16 ms render for 500 shapes) and note whether a virtualized list or Tauri-side pagination is required.
- [ ] [Critic] Document the exact keyboard-shortcut conflict check: verify Cmd/Ctrl+Shift+]/[ are not already bound in the existing 7-mode shortcut map before implementation.
### 🔍 Architect & Critic Feedback (Commit a0e7122 - Score: 7/10)
- [ ] [Critic] Add type annotations and structured error handling (Result/Exception classes) to post_commit_critic.py and e2e_animate.py to match the project's Rust safety bar; add a minimal pytest or cargo-based smoke test for each.
- [ ] [Critic] Introduce a rotation or size-cap policy for critic_history.jsonl (e.g., keep last 200 entries, archive to critic_history_archive/), or move it to a git-ignored local path to prevent unbounded repo growth.
- [ ] [Critic] Justify the Python tooling choice in a short ADR (Architecture Decision Record) or migrate the critic pipeline to a Rust binary invoked via Tauri IPC, eliminating the dual-language dependency surface.
- [ ] [Critic] Create an ASCII or Figma wireframe for the Layers Panel (10c) showing eye/lock/drag-handle icon placement and dark-slate color tokens, and attach it to the CRITIQUE.md or a new UI_SPECS/ directory before implementation begins.
- [ ] [Critic] Add a CI gate that blocks merges when critic_history.jsonl exceeds a configurable line threshold or when post_commit_critic.py reports a score below 6/10.
- [ ] [Critic] Split Milestone 10 into branches 10a (p:bg + p:bgRef roundtrip), 10b (spTree z-order reordering with unknown-node preservation), and 10c (Layers Panel UI) and open three separate PRs with independent CI gates.
### 🔍 Architect & Critic Feedback (Commit 3654611 - Score: 7/10)
- [ ] [Critic] Justify or migrate the Python critic tooling (post_commit_critic.py, e2e_animate.py) to a Rust binary or Tauri IPC hook to eliminate the second runtime dependency in the Rust/Tauri codebase.
- [ ] [Critic] Split Milestone 10 into three sub-milestones (10a: background engine, 10b: z-order reordering, 10c: layers panel) to enable incremental CI gating and shorter-lived branches.
- [ ] [Critic] Add a performance budget and virtualization strategy (e.g., windowed rendering at 200 visible rows) to the layer panel spec to prevent webview stalls on 500+ shape slides.
- [ ] [Critic] Add a roundtrip test fixture containing p:bgRef, blipFill, and a 200-shape spTree with unknown sibling nodes (p:grpSp, p:cxnSp) to the Rust test suite before implementation begins.
- [ ] [Critic] Include a rendered UI screenshot or DOM snapshot in future critic commits that touch UI-adjacent documentation to enable non-vacuous visual review.
- [ ] [Critic] Add visual acceptance criteria (pixel dimensions, color tokens, spacing values) to the layer panel TODO entries in TODO.md.
### 🔍 Architect & Critic Feedback (Commit f9141f3 - Score: 5/10)
- [ ] [Critic] Replace the Python HTTP-based critic with a native Rust binary or Tauri-side IPC hook to eliminate the 180s network timeout failure mode
- [ ] [Critic] Split b8faa729 into separate commits: one for Rust core (inspect.rs, document.rs, roundtrip.rs) and one for frontend (audience.html, index.html, slide_render.js, main.js)
- [ ] [Critic] Add a roundtrip test fixture in roundtrip.rs that includes p:bgRef, blipFill, and a 200-shape spTree with unknown sibling nodes (p:grpSp, p:cxnSp) to verify lossless preservation
- [ ] [Critic] Attach a rendered screenshot or DOM snapshot to any commit touching audience.html or index.html for visual regression gating
- [ ] [Critic] Add a performance assertion in slide_render.js (or a Rust-side benchmark) that renders a 500-shape slide within 16ms per frame in the Tauri webview
- [ ] [Critic] Define visual acceptance criteria (pixel dimensions, color tokens, spacing values) for the audience view and presenter console in TODO.md
- [ ] [Critic] Add a CI gate that fails the build if critic_history.jsonl's latest entry contains a timeout or error string instead of a valid score
### 🔍 Architect & Critic Feedback (Commit d55f70e - Score: 5/10)
- [ ] [Critic] Replace the Python HTTP-based critic with a native Rust binary or Tauri-side IPC hook to eliminate the 180s network timeout failure mode and second-runtime dependency
- [ ] [Critic] Split b8faa729 into separate commits: one for Rust core (inspect.rs, document.rs, roundtrip.rs, lib.rs) and one for frontend (audience.html, index.html, slide_render.js)
- [ ] [Critic] Add a roundtrip regression test in roundtrip.rs that explicitly asserts unknown XML nodes, namespaces, and relationships survive a full serialize→deserialize cycle
- [ ] [Critic] Attach a rendered screenshot or DOM snapshot to every commit touching HTML/JS files to enable visual verification in the critic pipeline
- [ ] [Critic] Define and document a performance budget for slide_render.js (e.g., <16ms frame time for 500+ shape slides) and add a benchmark harness
- [ ] [Critic] Audit audience.html and index.html for adherence to the project's dark-slate design-token system; replace any ad-hoc inline styles with CSS custom properties
- [ ] [Critic] Add responsive layout breakpoints for the audience view to handle window resizing in the Tauri webview
- [ ] [Critic] Ensure critic_history.jsonl entries include a structured error object (status, error_type, retry_count) when the pipeline fails, rather than a bare timeout string
### 🔍 Architect & Critic Feedback (Commit 6136a87 - Score: 4/10)
- [ ] [Critic] Replace the Python HTTP critic pipeline with a native Rust binary (cargo run --bin critic) or Tauri IPC hook to eliminate the second-runtime dependency and 180s timeout failure mode
- [ ] [Critic] Manually review the ~500-line changes in b8faa729 (audience.html, index.html, slide_render.js, inspect.rs, roundtrip.rs, document.rs, lib.rs) for lossless OOXML roundtrip compliance, Rust safety (no unhandled unwraps), and UI/UX quality before the next feature commit
- [ ] [Critic] Add a CI gate that blocks merge if critic_history.jsonl shows a consecutive failure count ≥ 2, forcing a manual review fallback
- [ ] [Critic] Add a performance benchmark (criterion or Tauri webview FPS counter) for slide_render.js rendering 500+ shape slides and assert a 60 FPS budget
- [ ] [Critic] Split the b8faa729 changes into separate commits by concern (frontend UI, Rust core, roundtrip tests) to enable git bisect for regression isolation
- [ ] [Critic] Attach a rendered screenshot or Playwright snapshot to every commit touching audience.html, index.html, or slide_render.js for visual regression detection
- [ ] [Critic] Verify inspect.rs explicitly preserves unknown XML nodes, namespaces, and relationships via a roundtrip test that injects a foreign namespace and asserts byte-identical output
### 🔍 Architect & Critic Feedback (Commit 0bbaf4a - Score: 4/10)
- [ ] [Critic] Replace the Python HTTP server critic pipeline with a native Rust binary invoked via `cargo run --bin oma-critic` or a Tauri IPC command; remove the 192.168.66.232:8000 dependency entirely
- [ ] [Critic] Add a structured 'failed_run' schema to critic_history.jsonl (fields: error_class, retry_count, backoff_seconds, parent_commit) so audit-trail entries are machine-parseable even on failure
- [ ] [Critic] Split the ~500-line b8faa729 commit into at least 3 atomic commits (Rust OOXML layer, frontend HTML/JS, integration wiring) to restore git bisect viability
- [ ] [Critic] Add a unit test in inspect.rs that round-trips a .pptx containing unknown XML nodes, custom namespaces, and orphan relationships, asserting byte-for-byte preservation (Architectural Rule 1)
- [ ] [Critic] Introduce a headless Tauri webview screenshot harness (e.g., `cargo run --bin oma-snapshot -- --slide 1 --out /tmp/snap.png`) and attach the output to every critic run
- [ ] [Critic] Add a performance benchmark for slide_render.js rendering 500+ shapes using `performance.now()` deltas, with a 16ms/frame budget assertion in the test suite
- [ ] [Critic] Add a CI gate that blocks merge if critic_history.jsonl contains more than 1 consecutive failed_run entry, forcing pipeline repair before further commits
### 🔍 Architect & Critic Feedback (Commit 268d3fd - Score: 3/10)
- [ ] [Critic] Replace the Python HTTP server critic pipeline with a native Rust binary invoked via Tauri IPC or `cargo critic` subcommand to eliminate the second-runtime dependency and non-deterministic timeout
- [ ] [Critic] Manually review the ~500-line diff in b8faa729 (inspect.rs, roundtrip.rs, document.rs, lib.rs) for lossless OOXML roundtrip compliance: verify unknown XML nodes, namespaces, and relationships are preserved
- [ ] [Critic] Add a `#[cfg(test)]` roundtrip test in roundtrip.rs that asserts byte-for-byte or semantic equivalence of unknown XML elements through a parse→serialize cycle
- [ ] [Critic] Add a performance benchmark for slide_render.js rendering 500+ shape slides; enforce a 16ms frame budget and cap DOM node creation with virtualization or canvas fallback
- [ ] [Critic] Promote the 9 [Critic]-tagged TODO.md items to a blocking CI gate that prevents merge until each item is resolved and verified
- [ ] [Critic] Capture and commit a visual snapshot (screenshot or rendered HTML) of audience.html, index.html, and the presenter console after b8faa729 to verify Milestone 8 UI parity
- [ ] [Critic] Add a `cargo test --release` and `wasm-pack build` step to the critic pipeline so that Rust safety (no unhandled unwraps in production paths) and frontend build integrity are checked before any review is recorded
### 🔍 Architect & Critic Feedback (Commit 9f57864 - Score: 4/10)
- [ ] [Critic] Replace the Python HTTP-server critic with a native Rust binary or Tauri IPC endpoint to eliminate the 192.168.66.232:8000 external dependency that has caused three consecutive timeouts.
- [ ] [Critic] Change the critic_history.jsonl schema so that timeout/failure runs record score as null (or -1) instead of a misleading 7, and add a 'status' field ("ok" | "timeout" | "error").
- [ ] [Critic] Add a CI gate that fails the build if critic_history.jsonl contains more than one consecutive non-ok entry, forcing a manual review before merge.
- [ ] [Critic] Manually audit all substantive Rust and frontend changes since b8faa729 for lossless OOXML roundtrip compliance, unhandled unwrap/expect in production paths, and UI/UX quality (dark-slate palette, font fallbacks, spacing).
- [ ] [Critic] Add a health-check probe (e.g., a /healthz endpoint or TCP connect test) to the critic pipeline that retries with exponential backoff before logging a timeout, reducing false-failure noise.
### 🔍 Architect & Critic Feedback (Commit d9febf0 - Score: 6/10)
- [ ] [Critic] Replace the Python HTTP-server critic (192.168.66.232:8000) with a native Rust binary or Tauri IPC endpoint to eliminate the external dependency causing three consecutive timeouts.
- [ ] [Critic] Update critic_history.jsonl schema to record score as null (or -1) with a 'status' field ('ok' | 'timeout' | 'error') for non-successful runs instead of a misleading 7.
- [ ] [Critic] Add a CI gate that fails the build if critic_history.jsonl contains more than one consecutive non-ok entry, forcing a manual review before merge.
- [ ] [Critic] Manually audit all substantive Rust and frontend changes since b8faa729 for OOXML lossless roundtrip compliance, Rust safety (no unhandled unwraps), and UI quality (dark-slate aesthetics, typography, spacing).
- [ ] [Critic] Provide a UI snapshot in the next critic run so visual quality can be assessed, or document the reason for its absence in CRITIQUE.md.
### 🔍 Architect & Critic Feedback (Commit 92c4a5d - Score: 6/10)
- [ ] [Critic] Replace the Python HTTP server critic pipeline with a native Rust binary or Tauri IPC call to eliminate the external 192.168.66.232:8000 dependency
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (e.g., 'timeout', 'success', 'error') and use null for score on non-evaluated runs instead of a misleading numeric value
- [ ] [Critic] Add a CI gate or pre-commit hook that blocks merge if the critic pipeline returns a non-success status
- [ ] [Critic] Manually audit all Rust and frontend changes since b8faa729 for OOXML roundtrip compliance, unwrap/panic safety, and UI quality before the next milestone
- [ ] [Critic] Add a 3-strike alerting mechanism (e.g., Slack/webhook) that fires when the critic pipeline fails consecutively
- [ ] [Critic] Verify the full untruncated TODO.md diff to confirm all 7 new entries are non-duplicative and correctly linked to the CRITIQUE.md concerns
### 🔍 Architect & Critic Feedback (Commit 2b38e8c - Score: 6/10)
- [ ] [Critic] Replace the Python HTTP server dependency in the critic pipeline with a native Rust binary or Tauri IPC channel; remove the 192.168.66.23:8000 network call entirely
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (enum: 'evaluated', 'timeout', 'error') and set score to null when status != 'evaluated'; backfill existing entries
- [ ] [Critic] Implement a CI gate (e.g., GitHub Actions or cargo test hook) that fails the build if critic_history.jsonl shows 2+ consecutive non-'evaluated' entries, with a Slack/email alert
- [ ] [Critic] Manually audit all ~500+ lines of unreviewed Rust/frontend code since b8faa729 for OOXML roundtrip compliance, unwrap/panic safety, and UI quality before the next feature merge
- [ ] [Critic] Add a screenshot hash or base64 thumbnail reference to each critic_history.jsonl entry so visual regressions are traceable without requiring a separate UI snapshot commit
- [ ] [Critic] Verify the full TODO.md additions (8 lines) and CRITIQUE.md 'Actionable Tasks' section are complete by re-running the diff without truncation
### 🔍 Architect & Critic Feedback (Commit d33d77b - Score: 6/10)
- [ ] [Critic] Replace the Python HTTP server dependency (192.168.66.23:8000) with a native Rust/Tauri IPC channel or a local Rust binary for the critic pipeline
- [ ] [Critic] Introduce a 'status' field in critic_history.jsonl schema (e.g., 'timeout', 'success', 'error') and set score to null on non-success runs instead of recording a misleading numeric value
- [ ] [Critic] Add a CI gate or pre-commit hook that blocks merge if the critic pipeline has failed 2+ consecutive times, with a Slack/email escalation alert
- [ ] [Critic] Manually audit the ~500+ lines of unreviewed Rust/frontend code for OOXML roundtrip compliance (unknown node preservation, namespace handling) and Rust safety (unwrap/expect in production paths)
- [ ] [Critic] Resolve the commit-message vs CRITIQUE.md chain inconsistency (2b38e8c vs 92c4a5d4) to prevent automated tooling confusion
- [ ] [Critic] Capture and attach a full UI screenshot of the presenter console and slide sorter to the next critic run to validate dark-slate aesthetics and typography integrity
### 🔍 Architect & Critic Feedback (Commit 6e9eac1 - Score: 5/10)
- [ ] [Critic] Replace the Python HTTP server dependency (192.168.66.232:8000) with a native Rust binary or Tauri IPC channel for the critic pipeline, eliminating the network SPOF
- [ ] [Critic] Add a 'status' field to critic_history.jsonl entries (e.g., 'ok', 'timeout', 'error') and use a null score sentinel on failure instead of inheriting the prior run's score
- [ ] [Critic] Implement a circuit-breaker or escalation alert after 2 consecutive critic pipeline failures to prevent silent quality-gate death
- [ ] [Critic] Add a CI gate or pre-commit hook that blocks merge if the critic pipeline has not completed successfully for the target commit
- [ ] [Critic] Sanitize CRITIQUE.md generation so that on pipeline failure the file records a structured error block (timestamp, error class, retry count) rather than a raw traceback string
- [ ] [Critic] Schedule a manual full audit of all Rust modules (OOXML roundtrip, Tauri IPC, error typing) and frontend components (7 modes, presenter console, slide sorter, stage tools) to close the four-cycle review gap
### 🔍 Architect & Critic Feedback (Commit e29efc8 - Score: 6/10)
- [ ] [Critic] Replace the external Python HTTP critic server with a local Rust binary or Tauri-side command so the quality gate no longer depends on a LAN IP and 180 s timeout
- [ ] [Critic] Add a 'status' field to critic_history.jsonl entries (e.g. 'ok' | 'timeout' | 'error') and use a null score sentinel for failed runs instead of inheriting the prior score
- [ ] [Critic] Write a build-time script (Rust or Node) that extracts icon paths from upstream_assets/icons/Icons.qml and emits SVG/PNG assets consumable by the Tauri frontend
- [ ] [Critic] Add a CI pre-commit hook or GitHub Actions gate that blocks merge when CRITIQUE.md contains a raw traceback or when the critic pipeline returns non-zero
- [ ] [Critic] Verify OmaDeck branding consistency across the in-app header, About dialog, export metadata (core.xml dc:title), and Tauri bundle metadata (tauri.conf.json productName, identifier) — not just the HTML <title>
- [ ] [Critic] Add a roundtrip test that loads upstream_assets/examples/A tour of OmaShow.omashow through the Rust OOXML reader and asserts zero unknown-node loss
- [ ] [Critic] Annotate upstream_assets/docs/screenshots/ with a README noting they depict the C++/Qt6 tool and should be treated as functional-reference only, not pixel-targets for the Tauri UI
### 🔍 Architect & Critic Feedback (Commit 7d3373a - Score: 6/10)
- [ ] [Critic] Replace the LAN HTTP critic pipeline with a local Rust binary or Tauri-side subprocess that writes CRITIQUE.md and critic_history.jsonl without network dependency
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (e.g., 'completed' | 'timeout' | 'skipped') and backfill existing timeout entries with null scores
- [ ] [Critic] Write a build-time codegen step (or manual SVG export) to translate upstream_assets/icons/Icons.qml into SVG/PNG assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Add a roundtrip test harness (cargo test) that parses the committed .omashow example file and asserts zero unknown-node loss against the original XML
- [ ] [Critic] Introduce a pre-commit hook or CI gate that blocks merge if the critic pipeline has failed >2 consecutive cycles, with a manual override flag
- [ ] [Critic] Add a Playwright or Tauri-side screenshot regression test for index.html to catch layout shifts from branding or CSS changes
- [ ] [Critic] Remove or quarantine the raw Python traceback text from CRITIQUE.md history to restore the audit trail's readability
### 🔍 Architect & Critic Feedback (Commit 2b07274 - Score: 5/10)
- [ ] [Critic] Replace the LAN Python HTTP critic server with a local Rust binary or Tauri-side critic module to eliminate the 192.168.66.232:8000 dependency and enable hermetic CI
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (e.g. 'timeout', 'ok', 'error') and use null/-1 for score on non-genuine evaluations to prevent semantic corruption
- [ ] [Critic] Add a CI gate or pre-commit hook that blocks merge if the critic pipeline returns a non-200 or times out, preventing 4+ consecutive silent failures
- [ ] [Critic] Write a build-time codegen adapter (Rust proc-macro or Node script) to translate upstream_assets/icons/Icons.qml into SVG/PNG assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Break the chore-only commit loop: schedule a feature commit targeting at least one Milestone 8 item (presenter console, slide sorter, or stage tools) within the next 3 commits
- [ ] [Critic] Add a visual regression test (e.g. Playwright screenshot diff or Tauri webview snapshot) to the CI pipeline to guard against broken glyphs and layout shifts in the dark-slate UI
- [ ] [Critic] Sanitize CRITIQUE.md to strip the raw Python traceback and replace with a structured 'Pipeline Failure' section including timestamp, error class, and remediation owner
### 🔍 Architect & Critic Feedback (Commit 58ea511 - Score: 4/10)
- [ ] [Critic] Implement a circuit-breaker in the critic pipeline: after 3 consecutive HTTP timeouts, fall back to a local Rust-based heuristic scorer and mark JSONL entries with 'status: timeout_fallback'
- [ ] [Critic] Replace the hardcoded LAN IP (192.168.66.232:8000) with a configurable endpoint via environment variable or tauri.conf.json, defaulting to localhost
- [ ] [Critic] Add a CI gate (GitHub Actions / cargo test) that blocks merge if critic_history.jsonl's last 3 entries all carry a timeout sentinel
- [ ] [Critic] Write a roundtrip test harness (cargo test --test roundtrip) that loads the 136 KB .omashow example, serializes back to OOXML, and asserts byte-level or semantic equivalence
- [ ] [Critic] Create a build-time codegen step (build.rs or a separate cargo-x task) that transpiles upstream_assets/icons/Icons.qml into SVG/PNG assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (enum: 'ok', 'timeout', 'fallback') and update all downstream consumers to handle non-numeric scores gracefully
- [ ] [Critic] Schedule a manual code-review session to unblock the 5-cycle stall and advance at least one Milestone 8 feature (presenter console, slide sorter, or stage tools)
### 🔍 Architect & Critic Feedback (Commit 1edc7b7 - Score: 3/10)
- [ ] [Critic] Implement a circuit-breaker in the critic pipeline: after 3 consecutive HTTP timeouts, fall back to a local Rust-based critic binary and emit a 'status: timeout' sentinel in critic_history.jsonl instead of a synthetic score
- [ ] [Critic] Replace the hardcoded LAN IP (192.168.66.232:8000) with a Tauri-side Rust critic module or a local cargo-run binary to eliminate the external network dependency
- [ ] [Critic] Add a CI gate or pre-commit hook that blocks merge if the critic pipeline returns a non-200 status, preventing 5+ cycles of unreviewed accumulation
- [ ] [Critic] Add a 'status' field (enum: ok | timeout | error) to critic_history.jsonl schema and backfill existing entries with 'timeout' to remove semantic ambiguity from inherited scores
- [ ] [Critic] Break the documentation-recording loop: allocate the next 3 commits to production Rust/Tauri IPC/UI work targeting Milestone 8 feature parity (presenter console, slide sorter, stage tools)
- [ ] [Critic] Create a build-time codegen step (e.g., a small Rust binary or Node script) to translate upstream_assets/icons/Icons.qml into SVG/PNG assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Provide a rendered UI screenshot or Tauri window capture with each critic run to enable visual regression detection
### 🔍 Architect & Critic Feedback (Commit e53432e - Score: 3/10)
- [ ] [Critic] Replace the hardcoded LAN IP HTTP critic call with a local Rust binary (cargo run --bin omashow-critic) or Tauri IPC channel to eliminate the external dependency
- [ ] [Critic] Add a 'status' field to critic_history.jsonl schema (values: 'ok', 'timeout', 'error') so downstream tooling can distinguish genuine scores from synthetic fallbacks
- [ ] [Critic] Implement a circuit-breaker in the critic pipeline: after 3 consecutive timeouts, halt the auto-commit loop and emit a blocking CI failure
- [ ] [Critic] Add a pre-commit hook or CI gate that blocks merges when critic pipeline status is not 'ok' for more than 2 consecutive cycles
- [ ] [Critic] Write a roundtrip test harness (cargo test --test roundtrip) that parses the 136 KB .omashow example, serializes back to OOXML, and asserts byte-level or semantic equivalence
- [ ] [Critic] Create an SVG/PNG icon pipeline (build.rs codegen or a small Rust tool) to translate Icons.qml into assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Break the meta-circular loop: pause critic auto-commits and manually advance at least one production Rust module (e.g., slide sorter layout engine or presenter console IPC) before resuming the pipeline
### 🔍 Architect & Critic Feedback (Commit 5a68190 - Score: 3/10)
- [ ] [Critic] Replace the LAN-IP Python HTTP critic server with a local Rust binary (cargo run --bin oma-critic) or Tauri IPC invoke('critic_run') to eliminate the external dependency
- [ ] [Critic] Add a circuit-breaker + exponential-backoff (3 retries, 2s/4s/8s) to the critic pipeline; on final failure, write a JSONL entry with {"status": "timeout", "score": null} instead of a synthetic integer
- [ ] [Critic] Introduce a pre-commit hook or CI gate that blocks merge if critic_history.jsonl's last 3 entries are all non-null synthetic scores without a corresponding production diff
- [ ] [Critic] Write a roundtrip test: parse the 136 KB .omashow example → serialize → re-parse → assert byte-identical XML tree (lossless OOXML guarantee)
- [ ] [Critic] Add a build-time codegen step (build.rs or a small Rust proc-macro) to translate Icons.qml into SVG/PNG assets consumable by the Tauri/HTML5 frontend
- [ ] [Critic] Break the meta-cycle: halt critic-recording commits until at least one production Rust module or UI component lands in the next 3 commits
- [ ] [Critic] Add a 'pipeline_health' field to critic_history.jsonl schema and validate it with a JSON Schema or serde deserialization guard
