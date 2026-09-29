//! Standalone HTML5 export: structural checks on the generated page.

use omashow_core::PptxDocument;

fn count_occurrences(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

fn assert_html_structure(html: &str, expected_slides: usize) {
    assert!(html.starts_with("<!doctype html>"), "missing doctype");
    assert!(
        html.ends_with("</html>\n") || html.ends_with("</html>"),
        "missing closing </html>"
    );
    assert!(html.contains("<meta charset=\"utf-8\">"), "missing charset");
    assert!(html.contains("id=\"stage\""), "missing stage container");
    assert!(html.contains("id=\"counter\""), "missing slide counter");
    assert!(
        count_occurrences(html, "<section class=\"slide\">") == expected_slides,
        "wrong slide section count"
    );
}

#[test]
fn exports_one_section_per_slide() {
    let doc = PptxDocument::open("tests/fixtures/m365_sampleshow.bin").unwrap();
    let expected = doc.slide_count();
    assert!(expected >= 2);
    let html = String::from_utf8(doc.export_html_bytes().unwrap()).unwrap();
    assert_html_structure(&html, expected);
}

#[test]
fn new_deck_geometry_maps_to_96_dpi_px() {
    let mut doc = PptxDocument::new();
    doc.add_slide(Some("First".to_string())).unwrap();
    let html = String::from_utf8(doc.export_html_bytes().unwrap()).unwrap();
    assert_html_structure(&html, 1);
    // Default widescreen canvas: 12 192 000 x 6 858 000 EMU = 1280 x 720 px.
    assert!(
        html.contains("width:1280.00px;height:720.00px"),
        "stage px size wrong"
    );
}

#[test]
fn slide_text_is_escaped_and_present() {
    let mut doc = PptxDocument::new();
    doc.add_slide(Some("A & B <draft>".to_string())).unwrap();
    let html = String::from_utf8(doc.export_html_bytes().unwrap()).unwrap();
    assert_html_structure(&html, 1);
    assert!(
        html.contains("A &amp; B &lt;draft&gt;"),
        "title text must be HTML-escaped"
    );
    assert!(
        !html.contains("A & B <draft>"),
        "raw markup leaked into output"
    );
}

#[test]
fn picture_shapes_embed_their_data_uri() {
    let doc = PptxDocument::open("tests/fixtures/libreoffice_picture_transparency.bin").unwrap();
    let html = String::from_utf8(doc.export_html_bytes().unwrap()).unwrap();
    assert_html_structure(&html, doc.slide_count());
    assert!(
        count_occurrences(&html, "class=\"pic\"") > 0,
        "no picture elements emitted"
    );
    assert!(
        count_occurrences(&html, "src=\"data:image/") > 0,
        "picture data URIs not embedded"
    );
    // The bundle must be self-contained: no external http(s) resources.
    assert!(!html.contains("src=\"http"), "external resource reference");
    assert!(!html.contains("href=\"http"), "external resource reference");
}

#[test]
fn export_to_file_writes_html() {
    let doc = PptxDocument::open("tests/fixtures/m365_with_master.bin").unwrap();
    let path = std::env::temp_dir().join(format!(
        "omashow-export-{}-{}.html",
        std::process::id(),
        uuid_suffix()
    ));
    doc.export_html(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let html = String::from_utf8(bytes).unwrap();
    assert_html_structure(&html, doc.slide_count());
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
