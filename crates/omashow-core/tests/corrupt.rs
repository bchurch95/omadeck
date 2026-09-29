//! Corrupt PPTX handling: malformed packages must fail with a typed error,
//! never panic. The bad packages are synthesized at test time from the
//! corpus fixtures, so the suite stays self-contained.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use omashow_core::PptxDocument;

fn tmp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("omashow-corrupt-{name}"))
}

fn write_case(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = tmp_path(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn assert_rejects(name: &str, path: &Path) {
    let result = std::panic::catch_unwind(|| PptxDocument::open(path));
    match result {
        Ok(Ok(_)) => panic!("{name}: corrupt package was accepted"),
        Ok(Err(e)) => eprintln!("{name}: rejected as expected: {e}"),
        Err(_) => panic!("{name}: open panicked on corrupt package"),
    }
}

#[test]
fn empty_file_is_rejected() {
    assert_rejects("empty", &write_case("empty", &[]));
}

#[test]
fn garbage_bytes_are_rejected() {
    let bytes: Vec<u8> = (0u16..2048).map(|i| (i % 251) as u8).collect();
    assert_rejects("garbage", &write_case("garbage", &bytes));
}

#[test]
fn truncated_pptx_is_rejected() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m365_sampleshow.bin");
    let bytes = fs::read(&src).unwrap();
    // Cut the zip mid-central-directory: a reader must not walk past EOF.
    let cut = bytes.len() / 2;
    assert_rejects("truncated", &write_case("truncated", &bytes[..cut]));
}

#[test]
fn zip_without_content_types_is_rejected() {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file(
            "ppt/presentation.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"<x/>").unwrap();
        zip.finish().unwrap();
    }
    let buf = cursor.into_inner();
    assert_rejects("no-content-types", &write_case("no-content-types", &buf));
}

#[test]
fn invalid_content_types_xml_is_rejected() {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file(
            "[Content_Types].xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"<Types><Override this is not xml").unwrap();
        zip.finish().unwrap();
    }
    let buf = cursor.into_inner();
    assert_rejects("bad-content-types", &write_case("bad-content-types", &buf));
}

/// A valid package whose slide XML is destroyed must be rejected (or at
/// least never panic) when opened.
#[test]
fn corrupted_slide_xml_is_rejected() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m365_sampleshow.bin");
    let file = fs::File::open(&src).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut out = zip::ZipWriter::new(&mut cursor);
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let name = entry.name().to_string();
            out.start_file(&name, zip::write::SimpleFileOptions::default())
                .unwrap();
            if name == "ppt/slides/slide1.xml" {
                out.write_all(b"<p:sld xmlns:p='x' unclosed").unwrap();
            } else {
                std::io::copy(&mut entry, &mut out).unwrap();
            }
        }
        out.finish().unwrap();
    }
    let buf = cursor.into_inner();
    assert_rejects("bad-slide-xml", &write_case("bad-slide-xml", &buf));
}

#[test]
fn cleanup_tmp_cases() {
    for name in [
        "empty",
        "garbage",
        "truncated",
        "no-content-types",
        "bad-content-types",
        "bad-slide-xml",
    ] {
        fs::remove_file(tmp_path(name)).ok();
    }
}
