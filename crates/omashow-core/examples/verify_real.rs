//! Verify lossless save against a real python-pptx deck.
//!
//! Build the fixture first (a realistic 3-slide deck with title, bullets, notes
//! and an embedded picture):
//!
//!     uv run --with python-pptx python scripts/make_real_deck.py /tmp/omashow-rt/real.pptx
//!
//! Then run:
//!
//!     cargo run -p omashow-core --example verify_real
//!
//! Checks:
//! 1. An unmodified save is byte-for-byte identical to the input.
//! 2. An edited save preserves every original part (matched by name), and the
//!    result still parses as a valid OPC package (every part has a resolvable
//!    content type).
//! 3. The edited file opens in office-toolkit with the edit applied and the
//!    slide count intact.
//!
//! Exits non-zero if any check fails.
use std::io::Cursor;

use office_toolkit::powerpoint::Presentation;
use office_toolkit::OpenFile;
use omashow_core::{model_of, PptxDocument};
use opc_ooxml::Package;

const SRC: &str = "/tmp/omashow-rt/real.pptx";
const OUT_NOOP: &str = "/tmp/omashow-rt/real_noop.pptx";
const OUT_EDIT: &str = "/tmp/omashow-rt/real_edit.pptx";
const NEW_TITLE: &str = "Q3 Quarterly Review (edited)";

fn part_names(path: &str) -> Vec<String> {
    let b = std::fs::read(path).unwrap();
    // Package::read_from also verifies that every part has a resolvable content
    // type — a merged file with a dangling part would fail here.
    let p = Package::read_from(Cursor::new(&b)).unwrap();
    let mut v: Vec<String> = p.parts().map(|x| x.name.clone()).collect();
    v.sort();
    v
}

fn check(ok: bool, label: &str) {
    if ok {
        println!("PASS {label}");
    } else {
        eprintln!("FAIL {label}");
        std::process::exit(1);
    }
}

fn main() {
    if !std::path::Path::new(SRC).exists() {
        eprintln!("missing {SRC} — build the python-pptx fixture first (make_real.py)");
        std::process::exit(1);
    }

    // 1) No-op save must be byte-identical.
    let doc = PptxDocument::open(SRC).unwrap();
    doc.save(OUT_NOOP).unwrap();
    check(
        std::fs::read(SRC).unwrap() == std::fs::read(OUT_NOOP).unwrap(),
        "no-op save is byte-identical",
    );

    // 2) Edited save must preserve every original part and apply the edit.
    let mut doc = PptxDocument::open(SRC).unwrap();
    doc.set_title(0, NEW_TITLE).unwrap();
    doc.save(OUT_EDIT).unwrap();

    let before = part_names(SRC);
    let after = part_names(OUT_EDIT);
    let lost: Vec<&String> = before.iter().filter(|n| !after.contains(n)).collect();
    println!(
        "real deck: {} parts before, {} after",
        before.len(),
        after.len()
    );
    if lost.is_empty() {
        println!("PASS all original parts preserved");
    } else {
        for n in &lost {
            println!("  LOST: {n}");
        }
        std::process::exit(1);
    }

    // 3) The edited file must open in office-toolkit with the edit applied and
    //    the rest of the deck intact.
    let src_model = model_of(&Presentation::open_file(SRC).unwrap());
    let pres = Presentation::open_file(OUT_EDIT).unwrap();
    let model = model_of(&pres);
    check(
        model.slides[0].title.as_deref() == Some(NEW_TITLE),
        "title edit applied",
    );
    check(
        model.slides.len() == src_model.slides.len(),
        "slide count intact",
    );

    println!("verify_real: all checks passed");
}
