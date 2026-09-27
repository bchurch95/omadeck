# OmaDeck

Modern PowerPoint clone built in Rust.

## Architecture

- `crates/omashow-core` — pure Rust core, PPTX read/write via office-toolkit
- `apps/omashow-tauri` — Tauri desktop app with web frontend

## Getting started

Create a demo PPTX:
```bash
cargo run --example create_demo -p omashow-core
```

CLI usage:
```bash
cargo run -p omashow-cli -- new deck.pptx
cargo run -p omashow-cli -- list deck.pptx
cargo run -p omashow-cli -- export deck.pptx deck.json
```

Run Tauri app:
```bash
cd apps/omashow-tauri
cargo tauri dev
```

## Development

Project is a Cargo workspace. Core is pure Rust, UI is Tauri + web.

```bash
git init
git add .
git commit -m "Initial scaffold"
```


## Roadmap

- Phase 1: PPTX import/export
- Phase 2: Viewer
- Phase 3: Editor
- Phase 4: Export PDF/PNG
