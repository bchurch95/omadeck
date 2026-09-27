//! Headless slide inspection: metadata, text runs, and bounding boxes —
//! including group-child remapping — verified in memory and through a real
//! PPTX file roundtrip.

use std::io::Cursor;

use omashow_core::{
    get_slide_shapes, slide_count, slide_dimensions, BoundingBox, LineInfo, PptxDocument,
};
use office_toolkit::drawing::{
    Color, Fill, Line, ShapeProperties, TextAlign, TextBody, TextParagraph, TextParagraphProperties,
    TextRun, TextRunProperties, Transform2D,
};
use office_toolkit::powerpoint::{
    AutoShape, Picture, PictureFormat, Placeholder, PlaceholderKind, Presentation, Shape,
    ShapeGroup, Slide,
};

fn title_body() -> TextBody {
    let p0 = TextParagraph::new()
        .with_run(TextRun::Regular {
            text: "Hello ".to_string(),
            properties: TextRunProperties::new()
                .with_bold(true)
                .with_font_size_points(24.0)
                .with_font_family("Calibri"),
        })
        .with_run(TextRun::Regular {
            text: "World".to_string(),
            properties: TextRunProperties::new().with_italic(true),
        });
    let p1 = TextParagraph::new()
        .with_run(TextRun::Regular {
            text: "Second".to_string(),
            properties: TextRunProperties::new(),
        })
        .with_run(TextRun::LineBreak { properties: None })
        .with_run(TextRun::Regular {
            text: "line".to_string(),
            properties: TextRunProperties::new()
                .with_fill(Fill::Solid(Color::Rgb("FF0000".to_string()))),
        });
    let p2 = TextParagraph::new()
        .with_properties(TextParagraphProperties::new().with_alignment(TextAlign::Center))
        .with_run(TextRun::text("Centered"));
    TextBody::new().with_paragraph(p0).with_paragraph(p1).with_paragraph(p2)
}

/// A two-slide deck: a titled slide with a text box, a picture, and a
/// 2×-scaled group (child space 1000×500 mapped into a 2000×1000 box at
/// (1000000, 2000000)), plus an empty second slide.
fn fixture_deck() -> Presentation {
    let title = AutoShape::new(2, "Title")
        .with_text_box(true)
        .with_placeholder(Placeholder::new(PlaceholderKind::Title))
        .with_properties(
            ShapeProperties::new().with_transform(
                Transform2D::new()
                    .with_offset(1_000_000, 3_000_000)
                    .with_extent(4_000_000, 1_000_000),
            ),
        )
        .with_text_body(title_body());

    let box_shape = AutoShape::new(3, "Box")
        .with_text_box(true)
        .with_properties(
            ShapeProperties::new()
                .with_transform(Transform2D::new().with_offset(500, 300).with_extent(200, 100))
                .with_fill(Fill::Solid(Color::Rgb("00FF00".to_string())))
                .with_line(
                    Line::new()
                        .with_width_emu(12700)
                        .with_fill(Fill::Solid(Color::Rgb("0000FF".to_string()))),
                ),
        )
        .with_text_body(TextBody::new().with_paragraph(TextParagraph::new().with_run(
            TextRun::text("Box"),
        )));

    let pic = Picture::new(
        4,
        "pic",
        [0u8; 3],
        PictureFormat::Png,
        1_000_000,
        2_000_000,
    )
    .with_offset(10_000_000, 5_000_000)
    .with_shape_properties(
        ShapeProperties::new().with_line(
            Line::new().with_width_emu(25400).with_fill(Fill::Solid(Color::Rgb("000000".to_string()))),
        ),
    );

    let child = AutoShape::new(5, "Child").with_properties(
        ShapeProperties::new().with_transform(
            Transform2D::new().with_offset(100, 50).with_extent(100, 100),
        ),
    );
    let group = ShapeGroup::new(6, "Group")
        .with_transform(
            (1_000_000, 2_000_000),
            (2_000_000, 1_000_000),
            (0, 0),
            (1_000_000, 500_000),
        )
        .with_shape(Shape::AutoShape(child));

    let slide1 = Slide::new()
        .with_shape(Shape::AutoShape(title))
        .with_shape(Shape::AutoShape(box_shape))
        .with_shape(Shape::Picture(pic))
        .with_shape(Shape::Group(group));
    Presentation::new()
        .with_slide(slide1)
        .with_slide(Slide::new())
}

#[test]
fn deck_metadata() {
    let pres = fixture_deck();
    assert_eq!(slide_count(&pres), 2);
    let dims = slide_dimensions(&pres);
    assert_eq!((dims.width_emu, dims.height_emu), (12_192_000, 6_858_000));
    assert!((dims.width_inches() - 13.333).abs() < 0.01);
    assert!((dims.height_inches() - 7.5).abs() < 0.01);

    let custom = Presentation::new().with_slide_size(9_144_000, 6_858_000);
    let dims = slide_dimensions(&custom);
    assert_eq!((dims.width_emu, dims.height_emu), (9_144_000, 6_858_000));
}

#[test]
fn out_of_range_slide_errors() {
    let pres = fixture_deck();
    assert!(get_slide_shapes(&pres, 2).is_err());
}

fn assert_slide0_inspection(pres: &Presentation) {
    let shapes = get_slide_shapes(pres, 0).expect("slide 0 exists");
    assert_eq!(shapes.len(), 4);

    // Title placeholder: kind, role, bounds, and run properties.
    let title = &shapes[0];
    assert_eq!(title.kind, "autoshape");
    assert_eq!(title.placeholder, Some("title"));
    assert_eq!(
        title.bounds,
        Some(BoundingBox {
            x_emu: 1_000_000,
            y_emu: 3_000_000,
            width_emu: 4_000_000,
            height_emu: 1_000_000,
        })
    );
    assert_eq!(title.text.as_deref(), Some("Hello World\nSecond\nline\nCentered"));
    assert_eq!(title.runs.len(), 6);
    assert_eq!(title.runs[0].text, "Hello ");
    assert!(title.runs[0].bold);
    assert!(!title.runs[0].italic);
    assert_eq!(title.runs[0].font_size_pt, Some(24.0));
    assert_eq!(title.runs[0].font_family.as_deref(), Some("Calibri"));
    assert_eq!(title.runs[0].paragraph, 0);
    assert_eq!(title.runs[1].text, "World");
    assert!(title.runs[1].italic);
    assert_eq!(title.runs[1].font_size_pt, None);
    assert_eq!(title.runs[2].text, "Second");
    assert_eq!(title.runs[2].paragraph, 1);
    assert_eq!(title.runs[3].text, "\n");
    assert_eq!(title.runs[3].paragraph, 1);
    assert_eq!(title.runs[4].text, "line");
    // Colors: only the last run sets one.
    assert_eq!(title.runs[0].color, None);
    assert_eq!(title.runs[4].color.as_deref(), Some("#FF0000"));
    // Alignment: only paragraph 2 declares one.
    assert_eq!(title.runs[0].alignment, None);
    assert_eq!(title.runs[2].alignment, None);
    assert_eq!(title.runs[5].text, "Centered");
    assert_eq!(title.runs[5].paragraph, 2);
    assert_eq!(title.runs[5].alignment.as_deref(), Some("center"));

    // Text box.
    let box_shape = &shapes[1];
    assert_eq!(box_shape.kind, "autoshape");
    assert_eq!(box_shape.placeholder, None);
    assert_eq!(
        box_shape.bounds,
        Some(BoundingBox {
            x_emu: 500,
            y_emu: 300,
            width_emu: 200,
            height_emu: 100,
        })
    );
    assert_eq!(box_shape.text.as_deref(), Some("Box"));
    assert_eq!(box_shape.runs.len(), 1);
    assert_eq!(box_shape.fill.as_deref(), Some("#00FF00"));
    assert_eq!(
        box_shape.line.as_ref(),
        Some(&LineInfo {
            width_emu: Some(12_700),
            color: Some("#0000FF".to_string()),
        })
    );

    // Picture.
    let pic = &shapes[2];
    assert_eq!(pic.kind, "picture");
    assert_eq!(
        pic.bounds,
        Some(BoundingBox {
            x_emu: 10_000_000,
            y_emu: 5_000_000,
            width_emu: 1_000_000,
            height_emu: 2_000_000,
        })
    );
    assert_eq!(pic.text, None);
    assert_eq!(pic.fill, None);
    assert_eq!(
        pic.line.as_ref(),
        Some(&LineInfo {
            width_emu: Some(25_400),
            color: Some("#000000".to_string()),
        })
    );

    // Group: own bounds, plus the child remapped 2× out of its 1000×500 space.
    let group = &shapes[3];
    assert_eq!(group.kind, "group");
    assert_eq!(
        group.bounds,
        Some(BoundingBox {
            x_emu: 1_000_000,
            y_emu: 2_000_000,
            width_emu: 2_000_000,
            height_emu: 1_000_000,
        })
    );
    let children = group.children.as_deref().expect("group has children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "Child");
    assert_eq!(
        children[0].bounds,
        Some(BoundingBox {
            x_emu: 1_000_200,
            y_emu: 2_000_100,
            width_emu: 200,
            height_emu: 200,
        })
    );

    // Empty second slide.
    assert!(get_slide_shapes(pres, 1).unwrap().is_empty());
}

#[test]
fn in_memory_inspection() {
    assert_slide0_inspection(&fixture_deck());
}

#[test]
fn inspection_survives_pptx_roundtrip() {
    let buf = fixture_deck()
        .write_to(Cursor::new(Vec::new()))
        .expect("serialize deck");
    let reopened = Presentation::read_from(Cursor::new(buf.get_ref())).expect("read deck back");
    assert_slide0_inspection(&reopened);
}

#[test]
fn pptx_document_inspection_methods() {
    let mut doc = PptxDocument::new();
    doc.add_slide(Some("First".to_string())).unwrap();
    doc.add_slide(None).unwrap();

    assert_eq!(doc.slide_count(), 2);
    let dims = doc.slide_dimensions();
    assert_eq!((dims.width_emu, dims.height_emu), (12_192_000, 6_858_000));

    let shapes = doc.get_slide_shapes(0).unwrap();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].placeholder, Some("title"));
    assert_eq!(shapes[0].text.as_deref(), Some("First"));

    assert!(doc.get_slide_shapes(9).is_err());
}

/// The CLI `inspect` command emits `get_slide_shapes` as JSON; make sure the
/// structure serializes cleanly and carries the fields a consumer needs
/// (runs, bounds, nested groups) for a multi-shape slide.
#[test]
fn slide_shapes_serialize_to_json() {
    let pres = fixture_deck();
    let value = serde_json::to_value(get_slide_shapes(&pres, 0).unwrap()).expect("serialize");

    let shapes = value.as_array().expect("shape array");
    assert_eq!(shapes.len(), 4);

    // Title: bold 24pt Calibri run, italic run, then "Second" + line break +
    // "line" in paragraph 1.
    let title = &shapes[0];
    assert_eq!(title["kind"], "autoshape");
    assert_eq!(title["placeholder"], "title");
    assert_eq!(title["text"], "Hello World\nSecond\nline\nCentered");
    assert_eq!(title["bounds"]["x_emu"], 1_000_000);
    assert_eq!(title["bounds"]["height_emu"], 1_000_000);
    let runs = title["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 6);
    assert_eq!(runs[0]["paragraph"], 0);
    assert_eq!(runs[0]["text"], "Hello ");
    assert_eq!(runs[0]["bold"], true);
    assert_eq!(runs[0]["font_size_pt"], 24.0);
    assert_eq!(runs[0]["font_family"], "Calibri");
    assert_eq!(runs[1]["text"], "World");
    assert_eq!(runs[1]["italic"], true);
    assert!(runs[1]["font_size_pt"].is_null());
    assert_eq!(runs[2]["text"], "Second");
    assert_eq!(runs[2]["paragraph"], 1);
    assert_eq!(runs[3]["text"], "\n");
    assert_eq!(runs[3]["paragraph"], 1);
    assert_eq!(runs[4]["text"], "line");
    assert!(runs[0]["color"].is_null());
    assert_eq!(runs[4]["color"], "#FF0000");
    assert!(runs[0]["alignment"].is_null());
    assert_eq!(runs[5]["text"], "Centered");
    assert_eq!(runs[5]["paragraph"], 2);
    assert_eq!(runs[5]["alignment"], "center");

    // Text box: bounds and text.
    let box_shape = &shapes[1];
    assert_eq!(box_shape["name"], "Box");
    assert_eq!(box_shape["bounds"]["x_emu"], 500);
    assert_eq!(box_shape["bounds"]["y_emu"], 300);
    assert_eq!(box_shape["bounds"]["width_emu"], 200);
    assert_eq!(box_shape["bounds"]["height_emu"], 100);
    assert_eq!(box_shape["runs"][0]["text"], "Box");
    assert_eq!(box_shape["fill"], "#00FF00");
    assert_eq!(box_shape["line"]["width_emu"], 12_700);
    assert_eq!(box_shape["line"]["color"], "#0000FF");

    // Picture: exact bounds, no text.
    let pic = &shapes[2];
    assert_eq!(pic["kind"], "picture");
    assert_eq!(pic["bounds"]["x_emu"], 10_000_000);
    assert_eq!(pic["bounds"]["y_emu"], 5_000_000);
    assert_eq!(pic["bounds"]["width_emu"], 1_000_000);
    assert_eq!(pic["bounds"]["height_emu"], 2_000_000);
    assert!(pic["text"].is_null());
    assert!(pic["runs"].as_array().unwrap().is_empty());
    assert!(pic["fill"].is_null());
    assert_eq!(pic["line"]["width_emu"], 25_400);
    assert_eq!(pic["line"]["color"], "#000000");

    // Group: own bounds, child remapped 2× out of its 1000×500 space.
    let group = &shapes[3];
    assert_eq!(group["kind"], "group");
    assert_eq!(group["bounds"]["x_emu"], 1_000_000);
    assert_eq!(group["bounds"]["y_emu"], 2_000_000);
    assert_eq!(group["bounds"]["width_emu"], 2_000_000);
    let children = group["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["name"], "Child");
    assert_eq!(children[0]["bounds"]["x_emu"], 1_000_200); // (100) * 2 + 1_000_000
    assert_eq!(children[0]["bounds"]["y_emu"], 2_000_100); // (50) * 2 + 2_000_000
    assert_eq!(children[0]["bounds"]["width_emu"], 200); // 100 * 2
    assert!(children[0]["text"].is_null());
}

/// Picture shapes carry the embedded media bytes: the data URI must
/// roundtrip through the PPTX media parts (`ppt/media/*`), and the
/// python-pptx-built real fixture must come back byte-for-byte.
#[test]
fn picture_media_data_roundtrips() {
    use base64::Engine as _;
    let png = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 96,
        248, 95, 15, 0, 2, 135, 1, 128, 235, 71, 186, 146, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
        96, 130,
    ];

    // In-memory fixture: a 3-byte "png" payload.
    let shapes = get_slide_shapes(&fixture_deck(), 0).expect("fixture shapes");
    let pic = shapes
        .iter()
        .find(|s| s.kind == "picture")
        .expect("fixture picture");
    let info = pic.pic.as_ref().expect("picture carries media data");
    assert_eq!(info.format, "png");
    assert_eq!(info.size_bytes, 3);
    let b64 = info
        .data_uri
        .strip_prefix("data:image/png;base64,")
        .expect("png data URI");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64"),
        [0u8; 3]
    );

    // Real fixture: slide 3 carries the PNG embedded by python-pptx.
    let doc = PptxDocument::open("/tmp/omashow-rt/real.pptx").expect("fixture opens");
    let shapes = doc.get_slide_shapes(2).expect("slide 3 shapes");
    let pic = shapes
        .iter()
        .find(|s| s.kind == "picture")
        .expect("embedded picture");
    let info = pic.pic.as_ref().expect("media data");
    assert_eq!(info.format, "png");
    assert_eq!(info.size_bytes, png.len());
    let b64 = info
        .data_uri
        .strip_prefix("data:image/png;base64,")
        .expect("png data URI");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64"),
        png,
        "embedded media must roundtrip byte-for-byte"
    );
}

#[test]
fn table_extraction_and_grid_info() {
    use office_toolkit::powerpoint::{SlideTable, TableCell, TableRow};

    let tb = |s: &str| TextBody::new().with_paragraph(TextParagraph::new().with_run(TextRun::text(s)));

    let cell1 = TableCell::new().with_text_body(tb("Header 1"));
    let cell2 = TableCell::new().with_text_body(tb("Header 2"));
    let row1 = TableRow::new(300_000).with_cell(cell1).with_cell(cell2);

    let cell3 = TableCell::new().with_text_body(tb("Data 1"));
    let cell4 = TableCell::new().with_text_body(tb("Data 2"));
    let row2 = TableRow::new(250_000).with_cell(cell3).with_cell(cell4);

    let table = SlideTable::new(99, "Test Table", 2_000_000, 550_000)
        .with_offset(500_000, 600_000)
        .with_column_widths(vec![1_000_000, 1_000_000])
        .with_row(row1)
        .with_row(row2)
        .with_style_first_row(true)
        .with_style_band_rows(true);

    let mut pres = Presentation::new();
    let slide = Slide::new().with_shape(Shape::Table(table));
    pres.slides.push(slide);

    let shapes = get_slide_shapes(&pres, 0).expect("slide shapes");
    assert_eq!(shapes.len(), 1);
    let shape = &shapes[0];
    assert_eq!(shape.kind, "table");
    assert_eq!(shape.name, "Test Table");

    let table_info = shape.table.as_ref().expect("table info exists");
    assert_eq!(table_info.column_widths_emu, vec![1_000_000, 1_000_000]);
    assert_eq!(table_info.row_heights_emu, vec![300_000, 250_000]);
    assert_eq!(table_info.rows.len(), 2);
    assert_eq!(table_info.rows[0].len(), 2);
    assert_eq!(table_info.rows[0][0].text.as_deref(), Some("Header 1"));
    assert_eq!(table_info.rows[0][1].text.as_deref(), Some("Header 2"));
    assert_eq!(table_info.rows[1][0].text.as_deref(), Some("Data 1"));
    assert_eq!(table_info.rows[1][1].text.as_deref(), Some("Data 2"));
    assert!(table_info.first_row);
    assert!(table_info.band_rows);
}

