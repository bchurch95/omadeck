//! Multi-vendor corpus: real-world PPTX packages must open, inspect, and
//! re-save losslessly. The fixtures in `tests/fixtures/` are unmodified
//! files from the Apache POI test suite (Apache-2.0) spanning PowerPoint
//! 2007–2016 (Windows + macOS) and LibreOffice 5–25. See
//! `tests/fixtures/README.md` for provenance and vendor-coverage notes.
//!
//! Files the upstream office-toolkit reader cannot yet parse (SmartArt
//! diagrams, legacy OLE objects) are tracked in `KNOWN_REJECTIONS`: they
//! must fail with a stable, typed error — never a panic.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use omashow_core::PptxDocument;

/// (fixture, expected slide count)
const FIXTURES: &[(&str, usize)] = &[
    ("tests/fixtures/m365_null_dates.bin", 1),
    ("tests/fixtures/m365_with_master.bin", 2),
    ("tests/fixtures/m365_themes.bin", 10),
    ("tests/fixtures/m365_sampleshow.bin", 2),
    ("tests/fixtures/m365_artistic_effects.bin", 2),
    ("tests/fixtures/m365_mac_key02.bin", 1),
    ("tests/fixtures/libreoffice_100610.bin", 17),
    ("tests/fixtures/libreoffice_picture_transparency.bin", 4),
];

/// (fixture, expected rejection message fragment). office-toolkit has no
/// model for SmartArt diagram parts or legacy OLE objects yet; the reader
/// must keep failing cleanly instead of panicking or silently accepting.
const KNOWN_REJECTIONS: &[(&str, &str)] = &[
    ("tests/fixtures/m365_smartart.bin", "not declared"),
    ("tests/fixtures/m365_bug64693.bin", "not declared"),
];

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

/// Map of `ppt/media/*` part name -> bytes inside a PPTX zip package.
fn media_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let file = fs::File::open(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut zip = zip::ZipArchive::new(file).expect("zip opens");
    let mut media = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().to_string();
        if name.starts_with("ppt/media/") {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
            media.insert(name, buf);
        }
    }
    media
}

/// Every corpus file must open, expose its expected slide count, and let
/// each slide be inspected without error. Shapes without explicit geometry
/// are legal (PowerPoint inherits placeholder bounds from the layout), so
/// absence of bounds is tolerated.
#[test]
fn corpus_opens_and_inspects() {
    for (name, expected_slides) in FIXTURES {
        let path = fixture_path(name);
        let doc = PptxDocument::open(&path)
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", path.display()));

        assert_eq!(
            doc.slide_count(),
            *expected_slides,
            "{}: unexpected slide count",
            path.display()
        );

        for i in 0..doc.slide_count() {
            let shapes = doc
                .get_slide_shapes(i)
                .unwrap_or_else(|e| panic!("{}: slide {i} inspect failed: {e}", path.display()));
            for sh in &shapes {
                if let Some(bounds) = &sh.bounds {
                    assert!(
                        bounds.width_emu >= 0 && bounds.height_emu >= 0,
                        "{}: slide {i} shape '{}' has negative bounds",
                        path.display(),
                        sh.name
                    );
                }
            }
        }
    }
}

/// Re-saving a corpus file must produce a package that reopens with the same
/// slide count and byte-identical embedded media.
#[test]
fn corpus_resave_is_lossless() {
    for (name, expected_slides) in FIXTURES {
        let path = fixture_path(name);
        let doc = PptxDocument::open(&path)
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", path.display()));

        let out = path.with_extension("resave.bin");
        doc.save(&out)
            .unwrap_or_else(|e| panic!("{}: save failed: {e}", path.display()));

        let reopened = PptxDocument::open(&out)
            .unwrap_or_else(|e| panic!("{}: resaved file failed to reopen: {e}", path.display()));
        assert_eq!(
            reopened.slide_count(),
            *expected_slides,
            "{}: slide count changed after resave",
            path.display()
        );

        let before = media_parts(&path);
        let after = media_parts(&out);
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "{}: media part set changed after resave",
            path.display()
        );
        for (k, v) in &before {
            assert_eq!(
                v,
                after.get(k).unwrap(),
                "{}: media bytes changed for {k}",
                path.display()
            );
        }

        fs::remove_file(&out).ok();
    }
}

/// Files with content the upstream reader does not model must be rejected
/// with a typed error containing the tracked message — never a panic and
/// never a silent accept.
#[test]
fn known_rejections_fail_cleanly() {
    for (name, needle) in KNOWN_REJECTIONS {
        let path = fixture_path(name);
        let result = std::panic::catch_unwind(|| PptxDocument::open(&path));
        match result {
            Ok(Ok(_)) => panic!("{name}: expected rejection, but the file opened"),
            Ok(Err(e)) => {
                let msg = e.to_string();
                assert!(
                    msg.contains(needle),
                    "{name}: rejection message drifted: {msg}"
                );
            }
            Err(_) => panic!("{name}: open panicked"),
        }
    }
}
