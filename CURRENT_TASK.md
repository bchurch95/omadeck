# CURRENT TASK: SmartArt Diagram Support (office-toolkit Reader Gap)

## Goal
Make SmartArt slides open instead of being rejected: office-toolkit mis-parses `dgm:relIds`
(diagram parts) as chart relationships and fails the package. Fix upstream (or vendor + patch)
so `m365_smartart.bin` opens, then promote it from `KNOWN_REJECTIONS` to `FIXTURES` in
`crates/omashow-core/tests/corpus.rs`.

## Context
- The rejection path is exercised today: `corpus.rs` pins `m365_smartart.bin` (and
  `m365_bug64693.bin` for OLE/`mc:AlternateContent`) as `KNOWN_REJECTIONS` with a stable,
  non-panicking error contract.
- The TODO item explicitly allows two routes: (a) upstream office-toolkit fix, or
  (b) vendor the crate and patch the relationship-kind resolution for `dgm` parts.

## Steps
1. Reproduce: `cargo run -p omashow-cli -- inspect crates/omashow-core/tests/fixtures/m365_smartart.bin`
   (or the corpus test) and capture the exact typed error.
2. Inspect the SmartArt package: `ppt/diagrams/*` parts and how `dgm:relIds` references them;
   identify where office-toolkit classifies the relationship as a chart.
3. Choose route:
   - Upstream: open issue/PR against office-toolkit with a minimal reproduction.
   - Vendor: copy the crate into the workspace, patch the rel-kind resolution, point
     `omashow-core` at the vendored path.
4. Promote `m365_smartart.bin` to `FIXTURES` in `corpus.rs` (rendering may degrade to an
   empty/placeholder shape — the contract is "opens without panicking"; note the render
   fidelity in the test).
5. Verify: `cargo test -p omashow-core`, `cargo check --workspace`, `cargo run -p omashow-core
   --example verify_real`.
6. Update `TODO.md` (mark the SmartArt item `[x]`), this file, and commit.

## Notes
- `m365_bug64693.bin` (legacy OLE / `mc:AlternateContent`) is the follow-up gap; model or
  gracefully skip `p:oleObj` so the rest of the slide renders.
- Do not exclude supervisor artifacts (CRITIQUE.md, watchdog.sh, critic_history.jsonl,
  supervisor_decisions.jsonl, .last_critiqued_commit, CRITIC_BACKLOG.md) from commits.
