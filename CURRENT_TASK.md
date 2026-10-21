# CURRENT TASK: Legacy OLE Objects / mc:AlternateContent (office-toolkit Reader Gap)

## Status
Previous task — SmartArt diagram support — is DONE:
- office-toolkit vendored under `vendor/powerpoint-ooxml` (0.24.1) + workspace `[patch.crates-io]`.
- `parse_chart` returns `Option<Chart>` (None for SmartArt diagram parts); `Slide` gains
  `smartarts: Vec<SmartArtShape>` (id/name/offset/extent); slide patcher preserves diagram
  parts and emits empty `dgm:relIds` placeholders; `Shape::SmartArt` handled in every
  `omashow-core` match site (lib.rs move/delete/search, inspect.rs).
- `m365_smartart.bin` promoted `KNOWN_REJECTIONS` → `FIXTURES` (1 slide); opens, inspects
  (kind "smartart" with bounds), and resaves losslessly. All omashow-core tests +
  `cargo check --workspace` + `verify_real` pass.

## Goal
Make `m365_bug64693.bin` open instead of being rejected: the deck contains a legacy OLE
object inside `mc:AlternateContent` that the upstream reader cannot classify. Model or
gracefully skip `p:oleObj` so the rest of the slide still renders; then promote the fixture
from `KNOWN_REJECTIONS` to `FIXTURES`.

## Context
- Rejection path pinned in `crates/omashow-core/tests/corpus.rs`:
  `("tests/fixtures/m365_bug64693.bin", "not declared")`.
- Same vendor+patch route as SmartArt: `vendor/powerpoint-ooxml` is already patched and
  workspace-wired via `[patch.crates-io]`.
- TODO.md item: "Legacy OLE objects / `mc:AlternateContent`: same rejection path
  (see `m365_bug64693.bin`). Model or gracefully skip `p:oleObj` so the rest of the slide
  still renders."

## Steps
1. Reproduce: `cargo run -p omashow-cli -- inspect crates/omashow-core/tests/fixtures/m365_bug64693.bin`
   and capture the exact typed error.
2. Inspect the package: locate the `mc:AlternateContent` / `p:oleObj` markup and find where
   the vendored reader rejects it.
3. Patch the vendored reader to model or skip the OLE shape without breaking other parts;
   mirror the SmartArt approach (new variant or skip) and handle every `omashow-core`
   match site.
4. Promote `m365_bug64693.bin` to `FIXTURES` in `corpus.rs` with the observed slide count.
5. Verify: `cargo test -p omashow-core`, `cargo check --workspace`,
   `cargo run -p omashow-core --example verify_real`.
6. Update `TODO.md` (mark the OLE item `[x]`), this file, and commit.

## Notes
- Do not exclude supervisor artifacts (CRITIQUE.md, watchdog.sh, critic_history.jsonl,
  supervisor_decisions.jsonl, .last_critiqued_commit, CRITIC_BACKLOG.md) from commits.
