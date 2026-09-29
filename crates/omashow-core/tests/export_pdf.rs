//! Vector PDF export: structural checks on the generated files.

use omashow_core::PptxDocument;

fn count_occurrences(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Page objects minus the page tree root.
fn pdf_page_count(bytes: &[u8]) -> usize {
    let c = |pat: &[u8]| count_occurrences(bytes, pat);
    (c(b"/Type /Page") + c(b"/Type/Page")) - (c(b"/Type /Pages") + c(b"/Type/Pages"))
}

fn assert_valid_pdf(bytes: &[u8], expected_pages: usize) {
    assert!(bytes.starts_with(b"%PDF-1."), "missing PDF header");
    assert!(
        bytes.ends_with(b"%%EOF") || bytes.ends_with(b"%%EOF\n"),
        "missing EOF marker"
    );
    assert!(
        count_occurrences(bytes, b"MediaBox") >= expected_pages,
        "missing MediaBox"
    );
    assert_eq!(pdf_page_count(bytes), expected_pages, "wrong page count");
}

#[test]
fn exports_one_page_per_slide() {
    let doc = PptxDocument::open("tests/fixtures/m365_sampleshow.bin").unwrap();
    assert_eq!(doc.slide_count(), 2);
    let bytes = doc.export_pdf_bytes().unwrap();
    assert_valid_pdf(&bytes, 2);
}

#[test]
fn exports_embedded_images() {
    let doc = PptxDocument::open("tests/fixtures/libreoffice_picture_transparency.bin").unwrap();
    let expected = doc.slide_count();
    assert!(expected >= 4);
    let bytes = doc.export_pdf_bytes().unwrap();
    assert_valid_pdf(&bytes, expected);
    assert!(
        count_occurrences(&bytes, b"/Subtype /Image")
            + count_occurrences(&bytes, b"/Subtype/Image")
            > 0,
        "no image XObjects embedded"
    );
}

#[test]
fn exports_multi_slide_deck_with_shapes() {
    let mut doc = PptxDocument::new();
    let a = doc.add_slide(Some("First".to_string())).unwrap();
    let _b = doc.add_slide(Some("Second".to_string())).unwrap();
    assert_eq!(a, 0);
    let bytes = doc.export_pdf_bytes().unwrap();
    assert_valid_pdf(&bytes, 2);
}

#[test]
fn export_to_file_writes_pdf() {
    let doc = PptxDocument::open("tests/fixtures/m365_with_master.bin").unwrap();
    let path = std::env::temp_dir().join(format!(
        "omashow-export-{}-{}.pdf",
        std::process::id(),
        uuid_suffix()
    ));
    doc.export_pdf(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_valid_pdf(&bytes, doc.slide_count());
    let _ = std::fs::remove_file(&path);
}

fn uuid_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
