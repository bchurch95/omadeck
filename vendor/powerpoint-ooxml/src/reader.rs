//! Reading a `.pptx` package into our [`Presentation`] model.
//!
//! A slide's own shapes (autoshapes, pictures, charts, groups, connectors, tables —
//! position/size/outline/fill/line, text, placeholder role), its speaker notes, its comments, and
//! the slide master's theme are resolved. The slide master/layout's own *shape* content (as opposed
//! to its theme) is still parsed by neither this function nor anything it calls (this crate has no
//! customizable model for the master/layout themselves yet, see `model.rs`'s doc comment).
//! Confirmed against real-world fixtures covering a single-slide deck, pictures/charts, notes,
//! groups, tables, and a multi-theme package — see this crate's tests. Connectors and comments
//! have no such fixture (none in this project's local corpus carries either) — see `model.rs`'s
//! top-level doc comment.

use std::collections::HashMap;
use std::io::{Read, Seek};

use drawing::{Fill, Geometry, PresetShape, ShapeProperties, TextAnchor, TextBody};
use opc::{Package, Relationships, TargetMode};
use xml_core::{BytesStart, Event, Reader};

use crate::error::{Error, Result};
use crate::model::{
    AutoShape, ColorScheme, Connector, CustomPropertyValue, DEFAULT_SLIDE_HEIGHT_EMU,
    DEFAULT_SLIDE_WIDTH_EMU, DocumentProperties, EmbeddedFont, FontScheme, MediaFormat, Picture,
    PictureFormat, Placeholder, PlaceholderKind, Presentation, Shape, ShapeConnection, ShapeGroup,
    SmartArt, Slide, SlideChart, SlideComment, SlideHyperlinkTarget, SlideMedia, SlideTable,
    SlideTableStyle,
    TableCell, TableCellProperties, TableRow, TableStylePart, Theme,
};

const OFFICE_DOCUMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const NOTES_SLIDE_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
// An internal slide-jump hyperlink's own relationship type, distinguishing it from
// `SlideHyperlinkTarget::External`'s (`"hyperlink"`) when resolving `<a:hlinkClick>`. Duplicated
// from `writer.rs`'s own constant of the same name (per-module constants, not shared between
// `reader.rs`/`writer.rs` — this crate's established convention).
const SLIDE_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const SLIDE_MASTER_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const THEME_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const COMMENT_AUTHORS_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/commentAuthors";
const COMMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const TABLE_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
/// `<a:graphicData uri="..">` value of a SmartArt diagram frame — distinguishes it from a chart
/// frame (same `<p:graphicFrame>` element, different payload) so the dispatch below doesn't hand
/// it to [`parse_chart`].
const DIAGRAM_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
const CORE_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const EXTENDED_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CUSTOM_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
// No `FONT_RELATIONSHIP_TYPE` constant here (unlike `writer.rs`, which needs it to *write* the
// relationship): `resolve_embedded_font_variant` below resolves an embedded font's `r:id` directly
// via `Relationships::by_id`, without ever filtering by relationship type.
const TABLE_STYLES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
// Same well-known "No Style, No Grid" GUID `writer.rs`'s own `TABLE_STYLE_GUID` uses — a table read
// back with exactly this `<a:tableStyleId>` text resolves to `SlideTable::style_id: None` (the
// fixed built-in fallback), not `Some("{5C22..}")`, preserving round-trip symmetry with a table
// this crate's own writer produced without an explicit `style_id`.
const TABLE_STYLE_GUID: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

impl Presentation {
    /// Reads a `.pptx` package from a seekable byte source.
    pub fn read_from<R: Read + Seek>(reader: R) -> Result<Self> {
        let package = Package::read_from(reader)?;

        let main_relationship = package
            .relationships()
            .iter()
            .find(|relationship| relationship.rel_type == OFFICE_DOCUMENT_RELATIONSHIP_TYPE)
            .ok_or(Error::MissingMainPresentation)?;
        let presentation_part_name = match main_relationship.target_mode {
            TargetMode::Internal => format!("/{}", main_relationship.target),
            TargetMode::External => return Err(Error::MissingMainPresentation),
        };
        let presentation_part = package
            .part(&presentation_part_name)
            .ok_or(Error::MissingMainPresentation)?;
        let xml = std::str::from_utf8(&presentation_part.data)
            .map_err(|error| Error::InvalidUtf8(presentation_part_name.clone(), error))?;

        let parsed = parse_presentation_xml(xml)?;

        let mut presentation = Presentation::new();
        presentation.slide_width_emu = parsed.slide_width_emu;
        presentation.slide_height_emu = parsed.slide_height_emu;
        presentation.theme = read_presentation_theme(&package, &presentation_part.relationships)?;

        // `ppt/tableStyles.xml`. Always present (this crate's own writer always writes it, even
        // with an empty `<a:tblStyleLst>`), but tolerant of it being missing regardless.
        if let Some(relationship) = presentation_part
            .relationships
            .iter()
            .find(|relationship| relationship.rel_type == TABLE_STYLES_RELATIONSHIP_TYPE)
        {
            let part_name = format!("/ppt/{}", relationship.target);
            if let Some(part) = package.part(&part_name) {
                let xml = std::str::from_utf8(&part.data)
                    .map_err(|error| Error::InvalidUtf8(part_name.clone(), error))?;
                presentation.table_styles = parse_table_styles_xml(xml)?;
            }
        }

        // Embedded fonts — `parse_presentation_xml` only collected raw `r:id`s (it has no access to
        // the package's own relationships/parts); resolve each to its actual `.fntdata` bytes here,
        // tolerant of any one relationship/part being missing (skips that variant rather than
        // erroring the whole presentation).
        for parsed_font in &parsed.embedded_fonts {
            presentation.embedded_fonts.push(EmbeddedFont {
                typeface: parsed_font.typeface.clone(),
                regular: resolve_embedded_font_variant(
                    &package,
                    &presentation_part.relationships,
                    &parsed_font.regular,
                ),
                bold: resolve_embedded_font_variant(
                    &package,
                    &presentation_part.relationships,
                    &parsed_font.bold,
                ),
                italic: resolve_embedded_font_variant(
                    &package,
                    &presentation_part.relationships,
                    &parsed_font.italic,
                ),
                bold_italic: resolve_embedded_font_variant(
                    &package,
                    &presentation_part.relationships,
                    &parsed_font.bold_italic,
                ),
            });
        }

        // `docProps/core.xml`/`app.xml`/`custom.xml`. Resolved off the *package root* relationships
        // (`_rels/.rels`), same as `ppt/presentation.xml` itself. Tolerant of any one
        // part/relationship being missing.
        presentation.properties = read_document_properties(&package)?;

        // `ppt/commentAuthors.xml` is package-wide, resolved once via `ppt/presentation.xml`'s own
        // relationships — every comment on every slide references it by numeric author id (see
        // `writer.rs`'s `CommentAuthorRegistry`'s own doc comment for the write side of this same
        // indirection).
        let comment_authors = match presentation_part
            .relationships
            .iter()
            .find(|relationship| relationship.rel_type == COMMENT_AUTHORS_RELATIONSHIP_TYPE)
        {
            Some(relationship) => {
                let part_name = format!("/ppt/{}", relationship.target);
                match package.part(&part_name) {
                    Some(part) => {
                        let xml = std::str::from_utf8(&part.data)
                            .map_err(|error| Error::InvalidUtf8(part_name.clone(), error))?;
                        parse_comment_authors_xml(xml)?
                    }
                    None => HashMap::new(),
                }
            }
            None => HashMap::new(),
        };

        for relationship_id in &parsed.slide_relationship_ids {
            let relationship = presentation_part
                .relationships
                .by_id(relationship_id)
                .ok_or_else(|| Error::MissingSlideRelationship(relationship_id.clone()))?;
            // `ppt/presentation.xml`'s relationships are relative to `ppt/` (e.g.
            // "slides/slide1.xml") — mirrors `word-ooxml`'s own `/word/` convention for
            // `word/document.xml.rels`.
            let slide_part_name = format!("/ppt/{}", relationship.target);
            let slide_part = package
                .part(&slide_part_name)
                .ok_or_else(|| Error::MissingSlidePart(slide_part_name.clone()))?;
            let slide_xml = std::str::from_utf8(&slide_part.data)
                .map_err(|error| Error::InvalidUtf8(slide_part_name.clone(), error))?;

            let context = SlideContext {
                slide_part_name: &slide_part_name,
                relationships: &slide_part.relationships,
                package: &package,
            };
            let mut slide = parse_slide_xml(slide_xml, &context)?;

            // A slide's speaker notes live in a *separate* part
            // (`ppt/notesSlides/notesSlideN.xml`), reached via a `notesSlide`-typed relationship on
            // the slide's own `.rels` — not embedded in the slide's own XML at all. Only present
            // when the slide actually has notes (see `Slide::notes`'s doc comment).
            if let Some(notes_relationship) = slide_part
                .relationships
                .iter()
                .find(|relationship| relationship.rel_type == NOTES_SLIDE_RELATIONSHIP_TYPE)
            {
                let notes_part_name =
                    resolve_relative_target(&slide_part_name, &notes_relationship.target);
                let notes_part = package
                    .part(&notes_part_name)
                    .ok_or_else(|| Error::MissingNotesPart(notes_part_name.clone()))?;
                let notes_xml = std::str::from_utf8(&notes_part.data)
                    .map_err(|error| Error::InvalidUtf8(notes_part_name.clone(), error))?;
                slide.notes = Some(parse_notes_slide_xml(notes_xml)?);
            }

            // A slide's comments live in their own part (`ppt/comments/commentN.xml`), reached via
            // a `comments`- typed relationship on the slide's own `.rels`, same "own separate part"
            // shape as notes. **Not grounded on a real fixture** — see `model.rs`'s top-level doc
            // comment.
            if let Some(comments_relationship) = slide_part
                .relationships
                .iter()
                .find(|relationship| relationship.rel_type == COMMENT_RELATIONSHIP_TYPE)
            {
                let comments_part_name =
                    resolve_relative_target(&slide_part_name, &comments_relationship.target);
                if let Some(comments_part) = package.part(&comments_part_name) {
                    let comments_xml = std::str::from_utf8(&comments_part.data)
                        .map_err(|error| Error::InvalidUtf8(comments_part_name.clone(), error))?;
                    slide.comments = parse_comments_xml(comments_xml, &comment_authors)?;
                }
            }

            presentation.slides.push(slide);
        }

        Ok(presentation)
    }
}

/// The pieces of `ppt/presentation.xml` this crate reads back.
struct ParsedPresentation {
    slide_width_emu: i64,
    slide_height_emu: i64,
    /// The `r:id` of each `<p:sldId>`, in `<p:sldIdLst>` (display) order.
    slide_relationship_ids: Vec<String>,
    /// `<p:embeddedFontLst>`'s own entries, `r:id`s not yet resolved to actual bytes (that needs
    /// the package/relationships this XML-only parser doesn't have access to — see `read_from`'s
    /// own resolution step).
    embedded_fonts: Vec<ParsedEmbeddedFont>,
}

/// One `<p:embeddedFont>` entry, `r:id`s not yet resolved.
struct ParsedEmbeddedFont {
    typeface: String,
    regular: Option<String>,
    bold: Option<String>,
    italic: Option<String>,
    bold_italic: Option<String>,
}

/// Parses `ppt/presentation.xml` (`CT_Presentation`): `<p:sldSz>`, the ordered list of slide
/// relationship ids from `<p:sldIdLst>`, and `<p:embeddedFontLst>`. Everything else
/// (`<p:sldMasterIdLst>`, `<p:notesMasterIdLst>`, `<p:notesSz>`, `<p:defaultTextStyle>`.) is
/// ignored — this crate has no model for any of it (the notes master is always the single fixed one
/// this crate's own writer produces, so there's nothing distinguishing to read back).
fn parse_presentation_xml(xml: &str) -> Result<ParsedPresentation> {
    let mut slide_width_emu = DEFAULT_SLIDE_WIDTH_EMU;
    let mut slide_height_emu = DEFAULT_SLIDE_HEIGHT_EMU;
    let mut slide_relationship_ids = Vec::new();
    let mut embedded_fonts = Vec::new();

    let mut reader = Reader::from_xml_str(xml);
    loop {
        match reader.read_event()? {
            Event::Eof => break,

            Event::Empty(start) if start.local_name().as_ref() == b"sldId" => {
                if let Some(relationship_id) = attribute_by_qname(&start, b"r:id") {
                    slide_relationship_ids.push(relationship_id);
                }
            }

            Event::Empty(start) if start.local_name().as_ref() == b"sldSz" => {
                if let Some(value) =
                    attribute_by_qname(&start, b"cx").and_then(|value| value.parse().ok())
                {
                    slide_width_emu = value;
                }
                if let Some(value) =
                    attribute_by_qname(&start, b"cy").and_then(|value| value.parse().ok())
                {
                    slide_height_emu = value;
                }
            }

            Event::Start(start) if start.local_name().as_ref() == b"embeddedFontLst" => {
                embedded_fonts = parse_embedded_font_list(&mut reader)?;
            }

            _ => {}
        }
    }

    Ok(ParsedPresentation {
        slide_width_emu,
        slide_height_emu,
        slide_relationship_ids,
        embedded_fonts,
    })
}

/// Parses `<p:embeddedFontLst>`'s content (a reader positioned right after its own opening tag was
/// consumed), stopping at `</p:embeddedFontLst>`.
fn parse_embedded_font_list(reader: &mut Reader) -> Result<Vec<ParsedEmbeddedFont>> {
    let mut fonts = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(start) if start.local_name().as_ref() == b"embeddedFont" => {
                fonts.push(parse_embedded_font(reader)?);
            }
            Event::End(end) if end.local_name().as_ref() == b"embeddedFontLst" => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(fonts)
}

/// Parses one `<p:embeddedFont>`'s content, assuming its own `Start` was just consumed by the
/// caller — consumes up to and including its own closing tag.
fn parse_embedded_font(reader: &mut Reader) -> Result<ParsedEmbeddedFont> {
    let mut typeface = String::new();
    let mut regular = None;
    let mut bold = None;
    let mut italic = None;
    let mut bold_italic = None;

    loop {
        match reader.read_event()? {
            Event::Empty(start) if start.local_name().as_ref() == b"font" => {
                typeface = attribute_by_qname(&start, b"typeface").unwrap_or_default();
            }
            Event::Empty(start) if start.local_name().as_ref() == b"regular" => {
                regular = attribute_by_qname(&start, b"r:id");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"bold" => {
                bold = attribute_by_qname(&start, b"r:id");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"italic" => {
                italic = attribute_by_qname(&start, b"r:id");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"boldItalic" => {
                bold_italic = attribute_by_qname(&start, b"r:id");
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::End(end) if end.local_name().as_ref() == b"embeddedFont" => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(ParsedEmbeddedFont {
        typeface,
        regular,
        bold,
        italic,
        bold_italic,
    })
}

/// The context a single slide's shapes are parsed against: its own part name (needed to resolve any
/// relative relationship target it carries, e.g. `./media/image1.png`) and its own `.rels`
/// relationships (images/ charts/notes), plus the whole package (to fetch the resolved parts'
/// bytes). Mirrors `word-ooxml`/`excel-ooxml`'s own per-part reading context.
struct SlideContext<'a> {
    slide_part_name: &'a str,
    relationships: &'a Relationships,
    package: &'a Package,
}

/// Resolves a relationship target that's relative to its owning part's own directory (e.g.
/// `./media/image1.png` from `ppt/slides/_rels/slide1.xml.rels`) into an absolute part name (e.g.
/// `/ppt/media/image1.png`).
///
/// Unlike `ppt/presentation.xml`'s own relationships (always resolved with the simple
/// `format!("/ppt/{}", target)` trick, reused above, because `ppt/presentation.xml` sits directly
/// inside `ppt/`), a slide's own relationships need real relative-path resolution: `ppt/slides/`
/// and `ppt/media/`/`ppt/charts/`/`ppt/notesSlides/` are *siblings* under `ppt/`, not parent/child,
/// so a target like `./media/image1.png` must walk back up one directory from `ppt/slides/` before
/// descending into `media/` — confirmed against real-world fixtures.
fn resolve_relative_target(base_part_name: &str, target: &str) -> String {
    let base_directory = match base_part_name.rfind('/') {
        Some(index) => &base_part_name[..index],
        None => "",
    };
    let mut segments: Vec<&str> = base_directory
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    format!("/{}", segments.join("/"))
}

/// Parses one `ppt/slides/slideN.xml` (`CT_Slide`) into a [`Slide`]: every
/// `<p:sp>`/`<p:pic>`/`<p:graphicFrame>`/`<p:grpSp>`/`<p:cxnSp>` in `<p:spTree>` becomes a
/// [`Shape`]. This is a flat, non-nesting-aware event scan (matching by local name wherever it
/// occurs in the document, not by tracking `<p:spTree>` depth) — safe because every shape-kind
/// parser this dispatches to fully consumes its own subtree (including any nested shapes inside a
/// `<p:grpSp>`, via [`parse_group_shape`]'s own recursive call back into this same per-shape
/// dispatch) before returning, so the reader's cursor never lands on an already-handled inner
/// element a second time. Speaker notes/comments aren't resolved here (they live in separate parts,
/// see `Presentation::read_from`).
fn parse_slide_xml(xml: &str, context: &SlideContext) -> Result<Slide> {
    let mut slide = Slide::new();
    let mut reader = Reader::from_xml_str(xml);
    let content = parse_shapes_until_eof(&mut reader, context)?;
    slide.shapes = content.shapes;
    slide.background = content.background;
    slide.hidden = content.hidden;
    slide.name = content.name;
    slide.hide_master_graphics = content.hide_master_graphics;
    Ok(slide)
}

/// Scans for top-level shape-starting events until EOF, dispatching each to its own parser — the
/// part shared by [`parse_slide_xml`] (the whole document) and [`parse_group_shape`] (a group's own
/// nested shape list, which stops at its own `</p:grpSp>` instead of EOF — see that function's own
/// loop). Also captures `<p:bg>` — `<p:cSld>`'s own first, optional child, harmless to detect via
/// this same flat scan since it can never itself contain a shape — and `<p:sld
/// show=".">`/`<p:cSld name=".">`'s own attributes, plus `<p:sld showMasterSp=".">`
/// (`<p:grpSp>` and every shape kind also happen to carry a `name` attribute of their own, but on
/// `<p:cNvPr>`, never bare `name` on the element itself the way `<p:cSld>` does — no ambiguity in
/// practice). This function is only ever called once per document, on the whole `<p:sld>`
/// (`<p:grpSp>`'s own nested shapes are scanned by [`parse_group_shape`]'s own, separate loop,
/// which has no use for `<p:sld>`/`<p:cSld>`'s attributes and so doesn't capture them). The
/// `<p:sld>`-level fields [`parse_shapes_until_eof`] collects in one EOF-terminated pass — grouped
/// into a struct instead of a 5-tuple so the return type stays readable at the call site (clippy's
/// `type_complexity` flags bare tuples this wide, and named fields also rule out silently
/// transposing two same-typed positions, e.g. the two `bool`s here).
struct ParsedSlideContent {
    shapes: Vec<Shape>,
    background: Option<Fill>,
    hidden: bool,
    name: Option<String>,
    hide_master_graphics: bool,
}

fn parse_shapes_until_eof(
    reader: &mut Reader,
    context: &SlideContext,
) -> Result<ParsedSlideContent> {
    let mut shapes = Vec::new();
    let mut background = None;
    let mut hidden = false;
    let mut name = None;
    let mut hide_master_graphics = false;
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"sld" => {
                hidden = attribute_by_qname(&start, b"show").as_deref() == Some("0");
                hide_master_graphics =
                    attribute_by_qname(&start, b"showMasterSp").as_deref() == Some("0");
            }
            Event::Start(start) if start.local_name().as_ref() == b"cSld" => {
                name = attribute_by_qname(&start, b"name").filter(|value| !value.is_empty());
            }
            Event::Start(start) if start.local_name().as_ref() == b"bg" => {
                background = parse_slide_background(reader)?;
            }
            Event::Start(start) if start.local_name().as_ref() == b"sp" => {
                shapes.push(Shape::AutoShape(parse_auto_shape(reader, Some(context))?));
            }
            Event::Start(start) if start.local_name().as_ref() == b"pic" => {
                shapes.push(parse_pic_shape(reader, context)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"graphicFrame" => {
                shapes.push(parse_graphic_frame_shape(reader, context)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"grpSp" => {
                shapes.push(Shape::Group(parse_group_shape(reader, context)?));
            }
            Event::Start(start) if start.local_name().as_ref() == b"cxnSp" => {
                shapes.push(Shape::Connector(parse_connector(reader)?));
            }
            _ => {}
        }
    }
    Ok(ParsedSlideContent {
        shapes,
        background,
        hidden,
        name,
        hide_master_graphics,
    })
}

/// Parses `<p:bg>`'s content (a reader positioned right after its own opening tag was consumed),
/// stopping at `</p:bg>` — only `<p:bgPr>`'s own fill choice is read back (`<p:bgRef>`, a theme
/// background-style reference, is not modeled — see [`crate::model::Slide::background`]'s own doc
/// comment).
fn parse_slide_background(reader: &mut Reader) -> Result<Option<Fill>> {
    let mut fill = None;
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"bg" => break,
            Event::Start(start) if start.local_name().as_ref() == b"bgPr" => {
                fill = drawing::read_fill(reader)?;
            }
            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }
    Ok(fill)
}

/// Parses `<p:grpSp>`'s content (`CT_GroupShape`) from a reader positioned right after the caller
/// already consumed its own opening tag. Recurses back into the same per-shape dispatch as
/// [`parse_shapes_until_eof`] for its own children (including nested groups), stopping at its own
/// `</p:grpSp>` instead of EOF. Confirmed against a real fixture with nested groups.
fn parse_group_shape(reader: &mut Reader, context: &SlideContext) -> Result<ShapeGroup> {
    let mut id: u32 = 0;
    let mut name = String::new();
    let mut offset_emu = (0i64, 0i64);
    let mut extent_emu = (0i64, 0i64);
    let mut child_offset_emu = (0i64, 0i64);
    let mut child_extent_emu = (0i64, 0i64);
    let mut rotation_60000ths: i32 = 0;
    let mut flip_horizontal = false;
    let mut flip_vertical = false;
    let mut shapes = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"grpSp" => break,

            Event::Start(start) if start.local_name().as_ref() == b"nvGrpSpPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"nvGrpSpPr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"cNvGrpSpPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"cNvGrpSpPr" => {
                // Can carry `<a:grpSpLocks/>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"nvPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"nvPr" => skip_subtree(reader)?,

            Event::Start(start) if start.local_name().as_ref() == b"grpSpPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"grpSpPr" => {}

            Event::Start(start) if start.local_name().as_ref() == b"xfrm" => {
                rotation_60000ths = attribute_by_qname(&start, b"rot")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                flip_horizontal = attribute_by_qname(&start, b"flipH").as_deref() == Some("1");
                flip_vertical = attribute_by_qname(&start, b"flipV").as_deref() == Some("1");
                let transform = parse_group_transform(reader)?;
                offset_emu = transform.offset;
                extent_emu = transform.extent;
                child_offset_emu = transform.child_offset;
                child_extent_emu = transform.child_extent;
            }

            // Nested shapes — same dispatch as the top-level `<p:spTree>`.
            Event::Start(start) if start.local_name().as_ref() == b"sp" => {
                shapes.push(Shape::AutoShape(parse_auto_shape(reader, Some(context))?));
            }
            Event::Start(start) if start.local_name().as_ref() == b"pic" => {
                shapes.push(parse_pic_shape(reader, context)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"graphicFrame" => {
                shapes.push(parse_graphic_frame_shape(reader, context)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"grpSp" => {
                shapes.push(Shape::Group(parse_group_shape(reader, context)?));
            }
            Event::Start(start) if start.local_name().as_ref() == b"cxnSp" => {
                shapes.push(Shape::Connector(parse_connector(reader)?));
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(ShapeGroup {
        id,
        name,
        offset_emu,
        extent_emu,
        child_offset_emu,
        child_extent_emu,
        rotation_60000ths,
        flip_horizontal,
        flip_vertical,
        shapes,
    })
}

/// The four `(x, y)`-shaped pairs [`parse_group_transform`] reads off a `<p:grpSpPr>`'s `<a:xfrm>`
/// — grouped into a struct instead of a 4-tuple-of-tuples (clippy's `type_complexity` flags nesting
/// this deep, and named fields also make it obvious which pair is the group's own on-slide extent
/// versus its children's coordinate-space extent, which a bare positional tuple doesn't).
struct GroupTransform {
    offset: (i64, i64),
    extent: (i64, i64),
    child_offset: (i64, i64),
    child_extent: (i64, i64),
}

/// Parses a `<p:grpSpPr>`'s own `<a:xfrm>` — `<a:off>`/`<a:ext>` (the group's own on-slide
/// position/size) plus `<a:chOff>`/`<a:chExt>` (the coordinate space its children's own `xfrm`
/// values are expressed in), from a reader positioned right after `<a:xfrm>`'s own opening tag was
/// consumed.
fn parse_group_transform(reader: &mut Reader) -> Result<GroupTransform> {
    let mut offset = (0i64, 0i64);
    let mut extent = (0i64, 0i64);
    let mut child_offset = (0i64, 0i64);
    let mut child_extent = (0i64, 0i64);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"xfrm" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"off" => {
                offset = read_xy_attributes(&start, b"x", b"y");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"ext" => {
                extent = read_xy_attributes(&start, b"cx", b"cy");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"chOff" => {
                child_offset = read_xy_attributes(&start, b"x", b"y");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"chExt" => {
                child_extent = read_xy_attributes(&start, b"cx", b"cy");
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(GroupTransform {
        offset,
        extent,
        child_offset,
        child_extent,
    })
}

/// Reads a two-attribute empty element's values as `(i64, i64)`, e.g. `<a:off x="1" y="2"/>` with
/// `first = b"x"`, `second = b"y"`.
fn read_xy_attributes(start: &BytesStart, first: &[u8], second: &[u8]) -> (i64, i64) {
    let first_value = attribute_by_qname(start, first)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let second_value = attribute_by_qname(start, second)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    (first_value, second_value)
}

/// Parses `<p:cxnSp>`'s content (`CT_Connector`) from a reader positioned right after the caller
/// already consumed its own opening tag. **Not grounded on a real fixture** — see `model.rs`'s
/// top-level doc comment.
fn parse_connector(reader: &mut Reader) -> Result<Connector> {
    let mut id: u32 = 0;
    let mut name = String::new();
    let mut properties = ShapeProperties::new();
    let mut start_connection = None;
    let mut end_connection = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"cxnSp" => break,

            Event::Start(start) if start.local_name().as_ref() == b"nvCxnSpPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"nvCxnSpPr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"cNvCxnSpPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"cNvCxnSpPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"cNvCxnSpPr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"stCxn" => {
                start_connection = Some(parse_shape_connection(&start));
            }
            Event::Empty(start) if start.local_name().as_ref() == b"endCxn" => {
                end_connection = Some(parse_shape_connection(&start));
            }

            Event::Empty(start) if start.local_name().as_ref() == b"nvPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"nvPr" => skip_subtree(reader)?,

            Event::Empty(start) if start.local_name().as_ref() == b"spPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"spPr" => {
                properties = drawing::read_shape_properties(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(Connector {
        id,
        name,
        properties,
        start_connection,
        end_connection,
    })
}

/// Reads a `<a:stCxn>`/`<a:endCxn>`'s `id`/`idx` attributes (`CT_Connection`).
fn parse_shape_connection(start: &BytesStart) -> ShapeConnection {
    ShapeConnection {
        shape_id: attribute_by_qname(start, b"id")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        index: attribute_by_qname(start, b"idx")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
    }
}

/// Parses `<p:sp>`'s content (`CT_Shape`) from a reader positioned right after the caller already
/// consumed its own opening tag — same contract as
/// `drawing::read_shape_properties`/`read_text_body`. `context` resolves a `<a:hlinkClick>` on this
/// shape's own `<p:cNvPr>` back to its real target URL; `None` when no relationship context is
/// available (e.g. [`parse_notes_slide_xml`]'s own placeholder shapes, which this crate never gives
/// a hyperlink anyway) — the hyperlink is then simply left unresolved (`None`), same forgiving
/// posture as the rest of this reader.
fn parse_auto_shape(reader: &mut Reader, context: Option<&SlideContext>) -> Result<AutoShape> {
    let mut id: u32 = 0;
    let mut name = String::new();
    let mut properties = ShapeProperties::new();
    let mut text_body = None;
    let mut placeholder = None;
    let mut is_text_box = false;
    let mut hyperlink = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"sp" => break,

            // `<p:nvSpPr>` is a transparent wrapper for `<p:cNvPr>`/ `<p:cNvSpPr>`/`<p:nvPr>` — its
            // own Start/End carry nothing to capture, so the loop simply falls through into its
            // children.
            Event::Start(start) if start.local_name().as_ref() == b"nvSpPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"nvSpPr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                // Real PowerPoint always nests an `<a:extLst>` (creation id) here, alongside an
                // optional `<a:hlinkClick>` — the latter is now resolved, the former still skipped
                // whole.
                hyperlink = parse_cnv_pr_hyperlink(reader, context)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"cNvSpPr" => {
                is_text_box = attribute_by_qname(&start, b"txBox").as_deref() == Some("1");
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvSpPr" => {
                is_text_box = attribute_by_qname(&start, b"txBox").as_deref() == Some("1");
                // Can carry `<a:spLocks../>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"nvPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"nvPr" => {
                placeholder = parse_placeholder_wrapper(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"spPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"spPr" => {
                properties = drawing::read_shape_properties(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"txBody" => {
                text_body = Some(drawing::read_text_body(reader)?);
            }

            // `<p:style>` and anything else this crate doesn't model.
            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}

            _ => {}
        }
    }

    Ok(AutoShape {
        id,
        name,
        properties,
        text_body,
        placeholder,
        is_text_box,
        hyperlink,
    })
}

/// Parses `<p:cNvPr>`'s remaining content (a reader positioned right after its own opening tag and
/// `id`/`name`/`descr` attributes were already consumed by the caller), looking for an optional
/// `<a:hlinkClick>` and resolving it to a [`SlideHyperlinkTarget`] against `context` (`None` if no
/// relationship context, or no matching relationship, is available). Consumes up to and including
/// `<p:cNvPr>`'s own closing tag.
fn parse_cnv_pr_hyperlink(
    reader: &mut Reader,
    context: Option<&SlideContext>,
) -> Result<Option<SlideHyperlinkTarget>> {
    let mut hyperlink = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"cNvPr" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"hlinkClick" => {
                hyperlink = resolve_hyperlink_target(&start, context);
            }
            Event::Start(start) if start.local_name().as_ref() == b"hlinkClick" => {
                hyperlink = resolve_hyperlink_target(&start, context);
                // Can nest `<a:extLst>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(hyperlink)
}

/// Resolves an `<a:hlinkClick>` into a [`SlideHyperlinkTarget`], covering both an external URL and
/// an in-package slide jump. Three cases, checked in order:
///
/// 1. No `r:id` at all, but `action="ppaction://hlinkshowjump?jump=.."` — one of the four
///    relative-jump variants, resolved purely from the `action` string, no relationship lookup
///    needed.
/// 2. `r:id` present, `action="ppaction://hlinksldjump"`, and the relationship it resolves to has
///    [`SLIDE_RELATIONSHIP_TYPE`] — an internal slide jump; its relative target
///    (`"./slides/slideN.xml"`) is parsed back into a 0-based [`SlideHyperlinkTarget::Slide`]
///    index.
/// 3. `r:id` present, anything else (or no `context`/no matching relationship at all) —
///    [`SlideHyperlinkTarget::External`], same as every hyperlink previously; falls back to `None`
///    when unresolvable, same forgiving posture as the rest of this reader (a shape with an
///    unresolvable hyperlink still reads back, just without one).
fn resolve_hyperlink_target(
    start: &BytesStart,
    context: Option<&SlideContext>,
) -> Option<SlideHyperlinkTarget> {
    let action = attribute_by_qname(start, b"action");
    let relationship_id = attribute_by_qname(start, b"r:id");

    if relationship_id.is_none() {
        return match action.as_deref() {
            Some("ppaction://hlinkshowjump?jump=nextslide") => {
                Some(SlideHyperlinkTarget::NextSlide)
            }
            Some("ppaction://hlinkshowjump?jump=previousslide") => {
                Some(SlideHyperlinkTarget::PreviousSlide)
            }
            Some("ppaction://hlinkshowjump?jump=firstslide") => {
                Some(SlideHyperlinkTarget::FirstSlide)
            }
            Some("ppaction://hlinkshowjump?jump=lastslide") => {
                Some(SlideHyperlinkTarget::LastSlide)
            }
            _ => None,
        };
    }

    let relationship = context?.relationships.by_id(&relationship_id?)?;

    if action.as_deref() == Some("ppaction://hlinksldjump")
        && relationship.rel_type == SLIDE_RELATIONSHIP_TYPE
    {
        if let Some(index) = parse_slide_relationship_target(&relationship.target) {
            return Some(SlideHyperlinkTarget::Slide(index));
        }
    }

    Some(SlideHyperlinkTarget::External(relationship.target.clone()))
}

/// Parses a slide relationship's own relative target (written by this crate's own writer as
/// `"./slides/slide{N}.xml"`, `N` 1-based) back into a 0-based slide index — `None` if it doesn't
/// match that exact shape (e.g. a hand-authored or differently-structured package).
fn parse_slide_relationship_target(target: &str) -> Option<usize> {
    let number = target
        .strip_prefix("../slides/slide")?
        .strip_suffix(".xml")?;
    let slide_number: usize = number.parse().ok()?;
    slide_number.checked_sub(1)
}

/// Parses `<p:nvPr>`'s content (a reader positioned right after its own opening tag was consumed)
/// looking for an optional `<p:ph type=".." idx="..">` (`CT_Placeholder`) child, returning `None`
/// when absent — e.g. `<p:nvPr/>` was already handled as `Event::Empty` by the caller and never
/// reaches this function at all.
fn parse_placeholder_wrapper(reader: &mut Reader) -> Result<Option<Placeholder>> {
    let mut placeholder = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"nvPr" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"ph" => {
                placeholder = Some(parse_placeholder_attributes(&start));
            }
            Event::Start(start) if start.local_name().as_ref() == b"ph" => {
                placeholder = Some(parse_placeholder_attributes(&start));
                // `<p:ph>` can carry an `<p:extLst>` child — not modeled.
                skip_subtree(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(placeholder)
}

/// Reads a `<p:ph>` element's `type`/`idx` attributes into a [`Placeholder`]. `type` defaults to
/// `"obj"` when absent, matching `CT_Placeholder`'s own schema default.
fn parse_placeholder_attributes(start: &BytesStart) -> Placeholder {
    let kind = attribute_by_qname(start, b"type")
        .map(|token| PlaceholderKind::from_xml_token(&token))
        .unwrap_or(PlaceholderKind::Object);
    let mut placeholder = Placeholder::new(kind);
    if let Some(index) = attribute_by_qname(start, b"idx").and_then(|value| value.parse().ok()) {
        placeholder = placeholder.with_index(index);
    }
    placeholder
}

/// Parses `<p:pic>`'s content (`CT_Picture`) from a reader positioned right after the caller
/// already consumed its own opening tag, returning either [`Shape::Picture`] or — when its own
/// `<p:nvPr>` carries an `<a:videoFile>`/`<a:audioFile>` reference — [`Shape::Media`]. Resolves the
/// relevant relationship(s) against `context` to fetch the actual media bytes (and, for a picture,
/// guesses its format from the resolved part name's extension).
fn parse_pic_shape(reader: &mut Reader, context: &SlideContext) -> Result<Shape> {
    let mut id: u32 = 0;
    let mut name = String::new();
    let mut description = String::new();
    let mut embed_relationship_id: Option<String> = None;
    let mut link_relationship_id: Option<String> = None;
    let mut media_relationship_id: Option<String> = None;
    let mut properties = ShapeProperties::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"pic" => break,

            Event::Start(start) if start.local_name().as_ref() == b"nvPicPr" => {}
            Event::End(end) if end.local_name().as_ref() == b"nvPicPr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                description = attribute_by_qname(&start, b"descr").unwrap_or_default();
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                description = attribute_by_qname(&start, b"descr").unwrap_or_default();
                // Can nest `<a:hlinkClick>`/`<a:extLst>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPicPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"cNvPicPr" => {
                // `<a:picLocks../>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"nvPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"nvPr" => {
                // A picture can itself be a placeholder (e.g. a notes slide's `sldImg`) — not
                // modeled on `Picture`/`SlideMedia`. A video/audio clip, however, carries its own
                // media relationship right here (`<a:videoFile>`/`<a:audioFile>`).
                media_relationship_id = parse_media_reference(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"blipFill" => {
                let (embed, link) = parse_blip_fill_embed(reader)?;
                embed_relationship_id = embed;
                link_relationship_id = link;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"spPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"spPr" => {
                properties = drawing::read_shape_properties(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    // Real position/size is `<a:xfrm>`'s own off/ext — never optional for a PresentationML picture
    // or media clip (see `Picture`'s doc comment) — pulled out of the parsed `transform` either
    // way.
    let offset_emu = properties
        .transform
        .as_ref()
        .and_then(|transform| transform.offset)
        .unwrap_or((0, 0));
    let extent_emu = properties
        .transform
        .as_ref()
        .and_then(|transform| transform.extent)
        .unwrap_or((0, 0));

    if let Some(media_relationship_id) = media_relationship_id {
        let relationship = context
            .relationships
            .by_id(&media_relationship_id)
            .ok_or_else(|| Error::MissingMediaRelationship(media_relationship_id.clone()))?;
        let media_part_name =
            resolve_relative_target(context.slide_part_name, &relationship.target);
        let media_part = context
            .package
            .part(&media_part_name)
            .ok_or_else(|| Error::MissingMediaPart(media_part_name.clone()))?;
        let format = MediaFormat::from_extension(&media_part_name)
            .ok_or_else(|| Error::UnsupportedMediaFormat(media_part_name.clone()))?;

        let poster_image = embed_relationship_id.and_then(|poster_relationship_id| {
            let relationship = context.relationships.by_id(&poster_relationship_id)?;
            let poster_part_name =
                resolve_relative_target(context.slide_part_name, &relationship.target);
            let poster_part = context.package.part(&poster_part_name)?;
            let poster_format = PictureFormat::from_extension(&poster_part_name)?;
            Some((poster_part.data.clone(), poster_format))
        });

        return Ok(Shape::Media(SlideMedia {
            id,
            name,
            data: media_part.data.clone(),
            format,
            offset_emu,
            extent_emu,
            poster_image,
        }));
    }

    properties.transform = None;

    let relationship_id =
        embed_relationship_id.ok_or_else(|| Error::MissingImageRelationship(String::new()))?;
    let relationship = context
        .relationships
        .by_id(&relationship_id)
        .ok_or_else(|| Error::MissingImageRelationship(relationship_id.clone()))?;
    let image_part_name = resolve_relative_target(context.slide_part_name, &relationship.target);
    let image_part = context
        .package
        .part(&image_part_name)
        .ok_or_else(|| Error::MissingImagePart(image_part_name.clone()))?;
    let format = PictureFormat::from_extension(&image_part_name)
        .ok_or_else(|| Error::UnsupportedImageFormat(image_part_name.clone()))?;

    // The rest (geometry/fill/line) is kept as `shape_properties` only when it differs from the
    // fixed default rectangle this crate's writer always produces for a plain picture.
    let is_default_shape = matches!(
        properties.geometry,
        None | Some(Geometry::Preset(PresetShape::Rectangle))
    ) && properties.fill.is_none()
        && properties.line.is_none();
    let shape_properties = if is_default_shape {
        None
    } else {
        Some(properties)
    };

    // `r:link`'s own relationship, when present, targets the external source directly
    // (`TargetMode::External` — its `target` *is* the URL/path, no part to resolve), unlike
    // `r:embed`'s internal, in-package relationship above.
    let external_link = link_relationship_id
        .and_then(|id| context.relationships.by_id(&id))
        .map(|relationship| relationship.target.clone());

    Ok(Shape::Picture(Picture {
        id,
        name,
        data: image_part.data.clone(),
        format,
        offset_emu,
        extent_emu,
        description,
        shape_properties,
        external_link,
    }))
}

/// Parses `<p:nvPr>`'s content (a reader positioned right after its own opening tag was consumed)
/// looking for an `<a:videoFile r:link=".">`/ `<a:audioFile r:link=".">`, returning its
/// relationship id — `None` for an ordinary picture (or a placeholder picture, e.g. a notes slide's
/// `sldImg`, neither of which carry either element). Consumes up to and including `<p:nvPr>`'s own
/// closing tag.
fn parse_media_reference(reader: &mut Reader) -> Result<Option<String>> {
    let mut relationship_id = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"nvPr" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"videoFile" => {
                relationship_id = attribute_by_qname(&start, b"r:link");
            }
            Event::Empty(start) if start.local_name().as_ref() == b"audioFile" => {
                relationship_id = attribute_by_qname(&start, b"r:link");
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(relationship_id)
}

/// Parses `<p:blipFill>`'s content (a reader positioned right after its own opening tag was
/// consumed) looking for `<a:blip>`'s `r:embed`/`r:link` relationship ids — `(embed, link)`.
/// `<a:srcRect>`/`<a:stretch>` are skipped, not modeled.
fn parse_blip_fill_embed(reader: &mut Reader) -> Result<(Option<String>, Option<String>)> {
    let mut embed_relationship_id = None;
    let mut link_relationship_id = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"blipFill" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"blip" => {
                embed_relationship_id = attribute_by_qname(&start, b"r:embed");
                link_relationship_id = attribute_by_qname(&start, b"r:link");
            }
            Event::Start(start) if start.local_name().as_ref() == b"blip" => {
                embed_relationship_id = attribute_by_qname(&start, b"r:embed");
                link_relationship_id = attribute_by_qname(&start, b"r:link");
                // Can nest `<a:extLst>` (e.g. `a14:useLocalDpi`) — not modeled.
                skip_subtree(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok((embed_relationship_id, link_relationship_id))
}

/// Parses `<p:graphicFrame>`'s content (`CT_GraphicalObjectFrame`), from a reader positioned right
/// after the caller already consumed its own opening tag. Wraps either a `<c:chart r:id="..">` or
/// an `<a:tbl>` — disambiguated by `<a:graphicData uri="..">`'s own `uri` attribute
/// (`CHART_NAMESPACE`/`TABLE_NAMESPACE`), the same discriminator real PowerPoint itself uses. Note
/// the frame's own transform element is `<p:xfrm>`, not `<a:xfrm>` — confirmed against real
/// fixtures covering both charts and tables — so it's parsed directly rather than via
/// `drawing::read_shape_properties` (which only knows about the `a:`-prefixed fragment).
fn parse_graphic_frame_shape(reader: &mut Reader, context: &SlideContext) -> Result<Shape> {
    let mut id: u32 = 0;
    let mut name = String::new();
    let mut offset_emu: (i64, i64) = (0, 0);
    let mut extent_emu: (i64, i64) = (0, 0);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicFrame" => break,

            Event::Start(start) if start.local_name().as_ref() == b"nvGraphicFramePr" => {}
            Event::End(end) if end.local_name().as_ref() == b"nvGraphicFramePr" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
            }
            Event::Start(start) if start.local_name().as_ref() == b"cNvPr" => {
                id = attribute_by_qname(&start, b"id")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                name = attribute_by_qname(&start, b"name").unwrap_or_default();
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"cNvGraphicFramePr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"cNvGraphicFramePr" => {
                // `<a:graphicFrameLocks../>` — not modeled.
                skip_subtree(reader)?;
            }

            Event::Empty(start) if start.local_name().as_ref() == b"nvPr" => {}
            Event::Start(start) if start.local_name().as_ref() == b"nvPr" => {
                skip_subtree(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"xfrm" => {
                let (parsed_offset, parsed_extent) = parse_frame_transform(reader)?;
                offset_emu = parsed_offset;
                extent_emu = parsed_extent;
            }

            // `<a:graphic>` is a transparent wrapper — fall through to its `<a:graphicData>` child.
            Event::Start(start) if start.local_name().as_ref() == b"graphic" => {}
            Event::End(end) if end.local_name().as_ref() == b"graphic" => {}

            // `<a:graphicData uri="..">`'s `uri` decides which payload follows — the rest of the
            // frame (everything up through its own `</p:graphicFrame>`) is delegated to a dedicated
            // parser per kind, each responsible for consuming up to that same end tag (mirrors
            // every other shape-kind parser's "consume up to my own closing tag" contract).
            Event::Start(start) if start.local_name().as_ref() == b"graphicData" => {
                let uri = attribute_by_qname(&start, b"uri");
                return match uri.as_deref() {
                    Some(TABLE_NAMESPACE) => Ok(Shape::Table(parse_table(
                        reader, id, name, offset_emu, extent_emu,
                    )?)),
                    Some(DIAGRAM_NAMESPACE) => Ok(Shape::SmartArt(parse_diagram(
                        reader, id, name, offset_emu, extent_emu,
                    )?)),
                    _ => Ok(Shape::Chart(Box::new(parse_chart(
                        reader, context, id, name, offset_emu, extent_emu,
                    )?))),
                };
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    // No `<a:graphicData>` was found at all (a malformed or genuinely empty frame) — defensive
    // fallback, same forgiving posture this reader takes elsewhere for optional/unexpected content:
    // an empty table rather than a hard error, since nothing about a bare `<p:graphicFrame>` is
    // inherently invalid to have read this far.
    Ok(Shape::Table(SlideTable {
        id,
        name,
        offset_emu,
        extent_emu,
        column_widths_emu: Vec::new(),
        rows: Vec::new(),
        style_id: None,
        style_first_row: false,
        style_first_column: false,
        style_last_row: false,
        style_last_column: false,
        style_band_rows: false,
        style_band_columns: false,
    }))
}

/// Parses a `<c:chart r:id="..">` payload (the reader positioned right after `<a:graphicData>`'s
/// own opening tag was consumed, `uri` already resolved to the chart namespace by the caller),
/// through to `</p:graphicFrame>`.
fn parse_chart(
    reader: &mut Reader,
    context: &SlideContext,
    id: u32,
    name: String,
    offset_emu: (i64, i64),
    extent_emu: (i64, i64),
) -> Result<SlideChart> {
    let mut chart_relationship_id: Option<String> = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicFrame" => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicData" => {}

            Event::Empty(start) if start.local_name().as_ref() == b"chart" => {
                chart_relationship_id = attribute_by_qname(&start, b"r:id");
            }
            Event::Start(start) if start.local_name().as_ref() == b"chart" => {
                chart_relationship_id = attribute_by_qname(&start, b"r:id");
                skip_subtree(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    let relationship_id =
        chart_relationship_id.ok_or_else(|| Error::MissingChartRelationship(String::new()))?;
    let relationship = context
        .relationships
        .by_id(&relationship_id)
        .ok_or_else(|| Error::MissingChartRelationship(relationship_id.clone()))?;
    let chart_part_name = resolve_relative_target(context.slide_part_name, &relationship.target);
    let chart_part = context
        .package
        .part(&chart_part_name)
        .ok_or_else(|| Error::MissingChartPart(chart_part_name.clone()))?;
    let chart_xml = std::str::from_utf8(&chart_part.data)
        .map_err(|error| Error::InvalidUtf8(chart_part_name.clone(), error))?;
    let chart_space = chart::read_chart_space(chart_xml)?;

    Ok(SlideChart {
        id,
        name,
        chart_space,
        offset_emu,
        extent_emu,
    })
}

/// Parses a SmartArt diagram frame payload (the reader positioned right after
/// `<a:graphicData>`'s own opening tag was consumed, `uri` already resolved to the diagram
/// namespace by the caller), through to `</p:graphicFrame>`. Unlike [`parse_chart`], the
/// diagram's own parts are never read: its `<dgm:relIds>` (or any other payload element) is
/// consumed and discarded, since the diagram data/layout parts encode a viewer-side layout
/// algorithm that this crate deliberately does not model. What survives is the frame's identity
/// and geometry — enough for a consumer to know a diagram is here and exactly where it sits.
fn parse_diagram(
    reader: &mut Reader,
    id: u32,
    name: String,
    offset_emu: (i64, i64),
    extent_emu: (i64, i64),
) -> Result<SmartArt> {
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicFrame" => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicData" => {}
            // `<dgm:relIds>` (possibly with children) and everything else is discarded.
            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(SmartArt {
        id,
        name,
        offset_emu,
        extent_emu,
    })
}

/// Parses an `<a:tbl>` payload (`CT_Table`, the reader positioned right after `<a:graphicData>`'s
/// own opening tag was consumed, `uri` already resolved to the table namespace by the caller),
/// through to `</p:graphicFrame>`. Confirmed against real fixtures.
fn parse_table(
    reader: &mut Reader,
    id: u32,
    name: String,
    offset_emu: (i64, i64),
    extent_emu: (i64, i64),
) -> Result<SlideTable> {
    let mut column_widths_emu = Vec::new();
    let mut rows = Vec::new();
    let mut style_id = None;
    let mut style_first_row = false;
    let mut style_first_column = false;
    let mut style_last_row = false;
    let mut style_last_column = false;
    let mut style_band_rows = false;
    let mut style_band_columns = false;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicFrame" => break,
            Event::End(end) if end.local_name().as_ref() == b"graphicData" => {}

            Event::Start(start) if start.local_name().as_ref() == b"tbl" => {}
            Event::End(end) if end.local_name().as_ref() == b"tbl" => {}

            // `<a:tblPr>` — wraps `<a:tableStyleId>` and carries the six style-application toggles
            // `firstRow`/`firstCol`/`lastRow`/`lastCol`/`bandRow`/`bandCol` (see
            // `SlideTable::style_first_row`'s doc comment — without these,
            // `style_id`/`SlideTableStyle` has no visible effect in real PowerPoint). The fixed
            // built-in GUID this crate's writer always falls back to resolves to `None`, see
            // `TABLE_STYLE_GUID`'s own doc comment here.
            Event::Empty(start) if start.local_name().as_ref() == b"tblPr" => {
                style_first_row = attribute_by_qname(&start, b"firstRow").as_deref() == Some("1");
                style_first_column =
                    attribute_by_qname(&start, b"firstCol").as_deref() == Some("1");
                style_last_row = attribute_by_qname(&start, b"lastRow").as_deref() == Some("1");
                style_last_column = attribute_by_qname(&start, b"lastCol").as_deref() == Some("1");
                style_band_rows = attribute_by_qname(&start, b"bandRow").as_deref() == Some("1");
                style_band_columns = attribute_by_qname(&start, b"bandCol").as_deref() == Some("1");
            }
            Event::Start(start) if start.local_name().as_ref() == b"tblPr" => {
                style_first_row = attribute_by_qname(&start, b"firstRow").as_deref() == Some("1");
                style_first_column =
                    attribute_by_qname(&start, b"firstCol").as_deref() == Some("1");
                style_last_row = attribute_by_qname(&start, b"lastRow").as_deref() == Some("1");
                style_last_column = attribute_by_qname(&start, b"lastCol").as_deref() == Some("1");
                style_band_rows = attribute_by_qname(&start, b"bandRow").as_deref() == Some("1");
                style_band_columns = attribute_by_qname(&start, b"bandCol").as_deref() == Some("1");
                style_id = parse_table_style_id(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"tblGrid" => {}
            Event::End(end) if end.local_name().as_ref() == b"tblGrid" => {}
            Event::Empty(start) if start.local_name().as_ref() == b"gridCol" => {
                if let Some(width) =
                    attribute_by_qname(&start, b"w").and_then(|value| value.parse().ok())
                {
                    column_widths_emu.push(width);
                }
            }
            Event::Start(start) if start.local_name().as_ref() == b"gridCol" => {
                if let Some(width) =
                    attribute_by_qname(&start, b"w").and_then(|value| value.parse().ok())
                {
                    column_widths_emu.push(width);
                }
                // Can carry an `<a:extLst>` (Office-internal column id) — not modeled.
                skip_subtree(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"tr" => {
                rows.push(parse_table_row(reader, &start)?);
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(SlideTable {
        id,
        name,
        offset_emu,
        extent_emu,
        column_widths_emu,
        rows,
        style_id,
        style_first_row,
        style_first_column,
        style_last_row,
        style_last_column,
        style_band_rows,
        style_band_columns,
    })
}

/// Parses `<a:tblPr>`'s content looking for `<a:tableStyleId>`'s text, assuming `<a:tblPr>`'s own
/// `Start` was just consumed by the caller — consumes up to and including `<a:tblPr>`'s own closing
/// tag. Returns `None` both when no `<a:tableStyleId>` is present and when its text matches the
/// fixed built-in GUID this crate's writer falls back to (see `TABLE_STYLE_GUID`'s own doc
/// comment).
fn parse_table_style_id(reader: &mut Reader) -> Result<Option<String>> {
    let mut style_id = None;
    let mut in_style_id = false;

    loop {
        match reader.read_event()? {
            Event::Start(start) if start.local_name().as_ref() == b"tableStyleId" => {
                in_style_id = true
            }
            Event::End(end) if end.local_name().as_ref() == b"tableStyleId" => in_style_id = false,
            Event::Text(text) if in_style_id => {
                let decoded = xml_core::decode_text(&text)?;
                if decoded != TABLE_STYLE_GUID {
                    style_id = Some(decoded);
                }
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::End(end) if end.local_name().as_ref() == b"tblPr" => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(style_id)
}

/// Parses `<a:tr h="..">`'s content (`CT_TableRow`) from a reader positioned right after its own
/// opening tag was consumed (`start` is that same opening tag, for its `h` attribute).
fn parse_table_row(reader: &mut Reader, start: &BytesStart) -> Result<TableRow> {
    let height_emu = attribute_by_qname(start, b"h")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut cells = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"tr" => break,

            Event::Start(start) if start.local_name().as_ref() == b"tc" => {
                cells.push(parse_table_cell(reader, &start)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"tc" => {
                cells.push(TableCell {
                    horizontal_span: attribute_by_qname(&start, b"gridSpan")
                        .and_then(|value| value.parse().ok()),
                    vertical_span: attribute_by_qname(&start, b"rowSpan")
                        .and_then(|value| value.parse().ok()),
                    horizontal_merge: attribute_by_qname(&start, b"hMerge").as_deref() == Some("1"),
                    vertical_merge: attribute_by_qname(&start, b"vMerge").as_deref() == Some("1"),
                    text_body: None,
                    properties: None,
                });
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(TableRow { height_emu, cells })
}

/// Parses `<a:tc>`'s content (`CT_TableCell`) from a reader positioned right after its own opening
/// tag was consumed (`start` is that same opening tag, for its
/// `gridSpan`/`rowSpan`/`hMerge`/`vMerge` attributes — see [`TableCell`]'s own doc comment for why
/// these live on `<a:tc>` itself rather than a nested properties element).
fn parse_table_cell(reader: &mut Reader, start: &BytesStart) -> Result<TableCell> {
    let horizontal_span =
        attribute_by_qname(start, b"gridSpan").and_then(|value| value.parse().ok());
    let vertical_span = attribute_by_qname(start, b"rowSpan").and_then(|value| value.parse().ok());
    let horizontal_merge = attribute_by_qname(start, b"hMerge").as_deref() == Some("1");
    let vertical_merge = attribute_by_qname(start, b"vMerge").as_deref() == Some("1");
    let mut text_body = None;
    let mut properties = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"tc" => break,

            Event::Start(start) if start.local_name().as_ref() == b"txBody" => {
                text_body = Some(drawing::read_text_body(reader)?);
            }

            // Cell margins/anchor/borders/fill (see [`TableCellProperties`]'s own doc comment).
            Event::Start(start) if start.local_name().as_ref() == b"tcPr" => {
                properties = Some(parse_table_cell_properties(reader, &start)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"tcPr" => {
                properties = Some(parse_table_cell_properties_attributes(&start));
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(TableCell {
        text_body,
        horizontal_span,
        vertical_span,
        horizontal_merge,
        vertical_merge,
        properties,
    })
}

/// Parses `<a:tcPr>`'s own attributes (`marL`/`marT`/`marR`/`marB`/ `anchor`) into a
/// [`TableCellProperties`] with every child field left unset — the `<a:tcPr/>` self-closing case,
/// which still carries attributes worth keeping even with no border/fill children.
fn parse_table_cell_properties_attributes(start: &BytesStart) -> TableCellProperties {
    TableCellProperties {
        margin_left_emu: attribute_by_qname(start, b"marL").and_then(|value| value.parse().ok()),
        margin_top_emu: attribute_by_qname(start, b"marT").and_then(|value| value.parse().ok()),
        margin_right_emu: attribute_by_qname(start, b"marR").and_then(|value| value.parse().ok()),
        margin_bottom_emu: attribute_by_qname(start, b"marB").and_then(|value| value.parse().ok()),
        anchor: attribute_by_qname(start, b"anchor")
            .and_then(|value| parse_table_cell_anchor(&value)),
        ..TableCellProperties::default()
    }
}

/// Parses `<a:tcPr>`'s element children (`<a:lnL>`/`<a:lnR>`/`<a:lnT>`/ `<a:lnB>` then the trailing
/// `EG_FillProperties` choice, point 8), assuming `<a:tcPr>`'s own `Start` was just consumed by the
/// caller — consumes up to and including `<a:tcPr>`'s own closing tag.
/// `<a:lnTlToBr>`/`<a:lnBlToTr>`/`<a:cell3D>` aren't modeled, see [`TableCellProperties`]'s own doc
/// comment.
fn parse_table_cell_properties(
    reader: &mut Reader,
    start: &BytesStart,
) -> Result<TableCellProperties> {
    let mut properties = parse_table_cell_properties_attributes(start);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"tcPr" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"lnL" => {
                properties.border_left = Some(drawing::read_line(reader, &start, false)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"lnL" => {
                properties.border_left = Some(drawing::read_line(reader, &start, true)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"lnR" => {
                properties.border_right = Some(drawing::read_line(reader, &start, false)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"lnR" => {
                properties.border_right = Some(drawing::read_line(reader, &start, true)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"lnT" => {
                properties.border_top = Some(drawing::read_line(reader, &start, false)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"lnT" => {
                properties.border_top = Some(drawing::read_line(reader, &start, true)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"lnB" => {
                properties.border_bottom = Some(drawing::read_line(reader, &start, false)?);
            }
            Event::Start(start) if start.local_name().as_ref() == b"lnB" => {
                properties.border_bottom = Some(drawing::read_line(reader, &start, true)?);
            }

            // The trailing `EG_FillProperties` choice — a bare sibling of `lnL`/`lnR`/`lnT`/`lnB`
            // above (no `<a:fill>` wrapper, unlike `CT_TableStyleCellStyle`'s own
            // `<a:tcStyle><a:fill>`), so `drawing::read_fill_choice` is used instead of
            // `drawing::read_fill` — see that function's own doc comment for why (it needs the
            // already-matched `start`/`is_start` rather than scanning forward itself, which would
            // silently skip past `lnL`/etc. if any came after it).
            Event::Start(start) if drawing::is_fill_choice_element(&start) => {
                properties.fill = Some(drawing::read_fill_choice(reader, &start, true)?);
            }
            Event::Empty(start) if drawing::is_fill_choice_element(&start) => {
                properties.fill = Some(drawing::read_fill_choice(reader, &start, false)?);
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(properties)
}

/// `ST_TextAnchoringType`'s own token parsing, duplicated locally — see `writer.rs`'s
/// `table_cell_anchor_token` for why (same `pub(crate)`-to- `drawing`-only visibility on the type's
/// own `from_xml_token`).
fn parse_table_cell_anchor(token: &str) -> Option<TextAnchor> {
    Some(match token {
        "t" => TextAnchor::Top,
        "ctr" => TextAnchor::Center,
        "b" => TextAnchor::Bottom,
        "just" => TextAnchor::Justified,
        "dist" => TextAnchor::Distributed,
        _ => return None,
    })
}

/// Parses `<p:xfrm>`'s `<a:off>`/`<a:ext>` children (a reader positioned right after `<p:xfrm>`'s
/// own opening tag was consumed), returning `((x, y), (cx, cy))` in EMUs.
fn parse_frame_transform(reader: &mut Reader) -> Result<((i64, i64), (i64, i64))> {
    let mut offset = (0i64, 0i64);
    let mut extent = (0i64, 0i64);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"xfrm" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"off" => {
                let x = attribute_by_qname(&start, b"x")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let y = attribute_by_qname(&start, b"y")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                offset = (x, y);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"ext" => {
                let cx = attribute_by_qname(&start, b"cx")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let cy = attribute_by_qname(&start, b"cy")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                extent = (cx, cy);
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok((offset, extent))
}

/// Parses a `ppt/notesSlides/notesSlideN.xml` part (root element `<p:notes>`, `CT_NotesSlide` —
/// note this differs from the part's own file name, confirmed against a real notes-bearing `.pptx`
/// fixture) back into the [`TextBody`] this crate's writer originally put in the `body` placeholder
/// (`idx="1"`). The `sldImg`/`sldNum` placeholder shapes real PowerPoint also writes are parsed
/// (via [`parse_auto_shape`], same as any other `<p:sp>`) but discarded — this crate never renders
/// slide thumbnails.
fn parse_notes_slide_xml(xml: &str) -> Result<TextBody> {
    let mut reader = Reader::from_xml_str(xml);
    let mut body_text: Option<TextBody> = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"sp" => {
                let shape = parse_auto_shape(&mut reader, None)?;
                let is_body_placeholder = matches!(
                    shape
                        .placeholder
                        .as_ref()
                        .map(|placeholder| &placeholder.kind),
                    Some(PlaceholderKind::Body)
                );
                if is_body_placeholder {
                    if let Some(text_body) = shape.text_body {
                        body_text = Some(text_body);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(body_text.unwrap_or_default())
}

/// Resolves and parses the slide master's own theme (`ppt/theme/theme1.xml` via
/// `ppt/slideMasters/slideMaster1.xml`'s own `.rels`), populating [`Presentation::theme`]. Returns
/// `None` (rather than erroring) if the master/theme chain can't be resolved for any reason — this
/// crate never hard-fails on the master/layout/theme boilerplate it doesn't otherwise validate (see
/// this module's own top-level doc comment), and a missing theme is no different: the writer will
/// just fall back to [`Theme::office_default`] next time this presentation is written.
fn read_presentation_theme(
    package: &Package,
    presentation_relationships: &Relationships,
) -> Result<Option<Theme>> {
    let Some(master_relationship) = presentation_relationships
        .iter()
        .find(|relationship| relationship.rel_type == SLIDE_MASTER_RELATIONSHIP_TYPE)
    else {
        return Ok(None);
    };
    // `ppt/presentation.xml`'s relationships are relative to `ppt/`, same convention as the slide
    // relationships resolved in `read_from`.
    let master_part_name = format!("/ppt/{}", master_relationship.target);
    let Some(master_part) = package.part(&master_part_name) else {
        return Ok(None);
    };
    let Some(theme_relationship) = master_part
        .relationships
        .iter()
        .find(|relationship| relationship.rel_type == THEME_RELATIONSHIP_TYPE)
    else {
        return Ok(None);
    };
    let theme_part_name = resolve_relative_target(&master_part_name, &theme_relationship.target);
    let Some(theme_part) = package.part(&theme_part_name) else {
        return Ok(None);
    };
    let xml = std::str::from_utf8(&theme_part.data)
        .map_err(|error| Error::InvalidUtf8(theme_part_name.clone(), error))?;
    Ok(Some(parse_theme_xml(xml)?))
}

/// Resolves one embedded-font variant's raw `r:id` (if any) to its actual `.fntdata` bytes via
/// `ppt/presentation.xml`'s own relationships. Returns `None` both when `raw_relationship_id`
/// itself is `None` and when the relationship/part it points at can't be resolved (tolerant, same
/// posture as the rest of this crate's reader).
fn resolve_embedded_font_variant(
    package: &Package,
    presentation_relationships: &Relationships,
    raw_relationship_id: &Option<String>,
) -> Option<Vec<u8>> {
    let relationship_id = raw_relationship_id.as_ref()?;
    let relationship = presentation_relationships.by_id(relationship_id)?;
    let part_name = format!("/ppt/{}", relationship.target);
    package.part(&part_name).map(|part| part.data.clone())
}

/// Resolves and parses `docProps/core.xml`/`app.xml`/`custom.xml` off the package root
/// relationships (`_rels/.rels`) into a [`DocumentProperties`]. Tolerant of any part/relationship
/// being missing, in which case the matching fields simply stay at their defaults.
fn read_document_properties(package: &Package) -> Result<DocumentProperties> {
    let mut properties = DocumentProperties::new();

    if let Some(relationship) = package
        .relationships()
        .iter()
        .find(|relationship| relationship.rel_type == CORE_PROPERTIES_RELATIONSHIP_TYPE)
    {
        let part_name = format!("/{}", relationship.target);
        if let Some(part) = package.part(&part_name) {
            let xml = std::str::from_utf8(&part.data)
                .map_err(|error| Error::InvalidUtf8(part_name.clone(), error))?;
            let core_properties = parse_core_properties_xml(xml)?;
            properties.title = core_properties.title;
            properties.author = core_properties.author;
            properties.subject = core_properties.subject;
            properties.keywords = core_properties.keywords;
        }
    }

    if let Some(relationship) = package
        .relationships()
        .iter()
        .find(|relationship| relationship.rel_type == EXTENDED_PROPERTIES_RELATIONSHIP_TYPE)
    {
        let part_name = format!("/{}", relationship.target);
        if let Some(part) = package.part(&part_name) {
            let xml = std::str::from_utf8(&part.data)
                .map_err(|error| Error::InvalidUtf8(part_name.clone(), error))?;
            let (company, manager, hyperlink_base) = parse_extended_properties_xml(xml)?;
            properties.company = company;
            properties.manager = manager;
            properties.hyperlink_base = hyperlink_base;
        }
    }

    if let Some(relationship) = package
        .relationships()
        .iter()
        .find(|relationship| relationship.rel_type == CUSTOM_PROPERTIES_RELATIONSHIP_TYPE)
    {
        let part_name = format!("/{}", relationship.target);
        if let Some(part) = package.part(&part_name) {
            let xml = std::str::from_utf8(&part.data)
                .map_err(|error| Error::InvalidUtf8(part_name.clone(), error))?;
            properties.custom_properties = parse_custom_properties_xml(xml)?;
        }
    }

    Ok(properties)
}

/// The four `docProps/core.xml` fields [`parse_core_properties_xml`] collects — grouped into a
/// struct instead of a same-typed 4-tuple (clippy's `type_complexity`; four `Option<String>`s in a
/// row is also exactly the kind of positional tuple that's easy to transpose).
struct ParsedCoreProperties {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
}

/// Parses `docProps/core.xml` into title/author/subject/keywords — an exact mirror of
/// `excel-ooxml`'s own `parse_core_properties_xml` (buffered text accumulation, `GeneralRef` entity
/// resolution included).
fn parse_core_properties_xml(xml: &str) -> Result<ParsedCoreProperties> {
    let mut reader = Reader::from_xml_str(xml);
    let mut title = None;
    let mut author = None;
    let mut subject = None;
    let mut keywords = None;
    let mut in_element: Option<&'static str> = None;
    let mut buffer = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"title" => {
                in_element = Some("title");
                buffer.clear();
            }
            Event::Start(start) if start.local_name().as_ref() == b"creator" => {
                in_element = Some("creator");
                buffer.clear();
            }
            Event::Start(start) if start.local_name().as_ref() == b"subject" => {
                in_element = Some("subject");
                buffer.clear();
            }
            Event::Start(start) if start.local_name().as_ref() == b"keywords" => {
                in_element = Some("keywords");
                buffer.clear();
            }
            Event::Text(text) if in_element.is_some() => {
                buffer.push_str(&xml_core::decode_text(&text)?);
            }
            Event::GeneralRef(reference) if in_element.is_some() => {
                if let Some(resolved) = xml_core::decode_general_ref(&reference)? {
                    buffer.push_str(&resolved);
                }
            }
            Event::End(end)
                if in_element.is_some()
                    && matches!(
                        end.local_name().as_ref(),
                        b"title" | b"creator" | b"subject" | b"keywords"
                    ) =>
            {
                let text = std::mem::take(&mut buffer);
                match in_element.take() {
                    Some("title") => title = Some(text),
                    Some("creator") => author = Some(text),
                    Some("subject") => subject = Some(text),
                    Some("keywords") => keywords = Some(text),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok(ParsedCoreProperties {
        title,
        author,
        subject,
        keywords,
    })
}

/// Parses `docProps/app.xml` into `(Company, Manager, HyperlinkBase)` — an exact mirror of
/// `excel-ooxml`'s own `parse_extended_properties_xml`.
fn parse_extended_properties_xml(
    xml: &str,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let mut reader = Reader::from_xml_str(xml);
    let mut company = None;
    let mut manager = None;
    let mut hyperlink_base = None;
    let mut in_element: Option<&'static str> = None;
    let mut buffer = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"Company" => {
                in_element = Some("Company");
                buffer.clear();
            }
            Event::Start(start) if start.local_name().as_ref() == b"Manager" => {
                in_element = Some("Manager");
                buffer.clear();
            }
            Event::Start(start) if start.local_name().as_ref() == b"HyperlinkBase" => {
                in_element = Some("HyperlinkBase");
                buffer.clear();
            }
            Event::Text(text) if in_element.is_some() => {
                buffer.push_str(&xml_core::decode_text(&text)?);
            }
            Event::GeneralRef(reference) if in_element.is_some() => {
                if let Some(resolved) = xml_core::decode_general_ref(&reference)? {
                    buffer.push_str(&resolved);
                }
            }
            Event::End(end)
                if in_element.is_some()
                    && matches!(
                        end.local_name().as_ref(),
                        b"Company" | b"Manager" | b"HyperlinkBase"
                    ) =>
            {
                let text = std::mem::take(&mut buffer);
                match in_element.take() {
                    Some("Company") => company = Some(text),
                    Some("Manager") => manager = Some(text),
                    Some("HyperlinkBase") => hyperlink_base = Some(text),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok((company, manager, hyperlink_base))
}

/// Parses `docProps/custom.xml` (OPC custom properties) into `(name, value)` pairs, in document
/// order — an exact mirror of `excel-ooxml`'s own `parse_custom_properties_xml` (including which 4
/// [`CustomPropertyValue`] variants are decoded).
fn parse_custom_properties_xml(xml: &str) -> Result<Vec<(String, CustomPropertyValue)>> {
    let mut reader = Reader::from_xml_str(xml);
    let mut custom_properties = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_variant: Option<Vec<u8>> = None;
    let mut buffer = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,

            Event::Start(start) if start.local_name().as_ref() == b"property" => {
                current_name = attribute_by_qname(&start, b"name");
            }

            Event::Start(start) | Event::Empty(start)
                if current_name.is_some()
                    && matches!(
                        start.local_name().as_ref(),
                        b"lpwstr" | b"bool" | b"i4" | b"r8"
                    ) =>
            {
                current_variant = Some(start.local_name().as_ref().to_vec());
                buffer.clear();
            }

            Event::Text(text) if current_variant.is_some() => {
                buffer.push_str(&xml_core::decode_text(&text)?);
            }

            Event::GeneralRef(reference) if current_variant.is_some() => {
                if let Some(resolved) = xml_core::decode_general_ref(&reference)? {
                    buffer.push_str(&resolved);
                }
            }

            Event::End(end) => {
                if let Some(variant) = current_variant.take() {
                    if end.local_name().as_ref() == variant.as_slice() {
                        if let Some(name) = current_name.clone() {
                            let value = std::mem::take(&mut buffer);
                            let parsed = match variant.as_slice() {
                                b"lpwstr" => Some(CustomPropertyValue::Text(value)),
                                b"bool" => {
                                    Some(CustomPropertyValue::Bool(value == "true" || value == "1"))
                                }
                                b"i4" => value.parse::<i32>().ok().map(CustomPropertyValue::Int),
                                b"r8" => value.parse::<f64>().ok().map(CustomPropertyValue::Number),
                                _ => None,
                            };
                            if let Some(value) = parsed {
                                custom_properties.push((name, value));
                            }
                        }
                    } else {
                        current_variant = Some(variant);
                    }
                } else if end.local_name().as_ref() == b"property" {
                    current_name = None;
                }
            }

            _ => {}
        }
    }

    Ok(custom_properties)
}

/// Parses `ppt/tableStyles.xml` (`CT_TableStyleList`) into custom [`SlideTableStyle`]s. Skips the
/// root's own `def` attribute entirely (this crate's writer always falls back to the same fixed
/// built-in GUID, see `TABLE_STYLE_GUID`'s own doc comment).
fn parse_table_styles_xml(xml: &str) -> Result<Vec<SlideTableStyle>> {
    let mut styles = Vec::new();

    let mut reader = Reader::from_xml_str(xml);
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"tblStyle" => {
                let id = attribute_by_qname(&start, b"styleId").unwrap_or_default();
                let name = attribute_by_qname(&start, b"styleName").unwrap_or_default();
                styles.push(parse_table_style(&mut reader, id, name)?);
            }
            Event::Empty(start) if start.local_name().as_ref() == b"tblStyle" => {
                let id = attribute_by_qname(&start, b"styleId").unwrap_or_default();
                let name = attribute_by_qname(&start, b"styleName").unwrap_or_default();
                styles.push(SlideTableStyle {
                    id,
                    name,
                    ..Default::default()
                });
            }
            _ => {}
        }
    }

    Ok(styles)
}

/// Parses one `<a:tblStyle>`'s content, assuming its own `Start` was just consumed by the caller —
/// consumes up to and including its own closing tag.
fn parse_table_style(reader: &mut Reader, id: String, name: String) -> Result<SlideTableStyle> {
    let mut style = SlideTableStyle {
        id,
        name,
        ..Default::default()
    };

    loop {
        match reader.read_event()? {
            Event::Start(start) => {
                let wrapper_name = start.local_name().as_ref().to_vec();
                let part = Some(parse_table_style_part(reader, &wrapper_name)?);
                match wrapper_name.as_slice() {
                    b"wholeTbl" => style.whole_table = part,
                    b"band1H" => style.band1_horizontal = part,
                    b"band2H" => style.band2_horizontal = part,
                    b"band1V" => style.band1_vertical = part,
                    b"band2V" => style.band2_vertical = part,
                    b"firstRow" => style.first_row = part,
                    b"lastRow" => style.last_row = part,
                    b"firstCol" => style.first_column = part,
                    b"lastCol" => style.last_column = part,
                    // `neCell`/`nwCell`/`seCell`/`swCell` (single-corner cells) — not modeled, see
                    // `SlideTableStyle`'s own doc comment for scope.
                    _ => {}
                }
            }
            Event::Empty(_) => {}
            Event::End(end) if end.local_name().as_ref() == b"tblStyle" => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(style)
}

/// Parses one table-part element's content (`<a:wholeTbl>`/`<a:band1H>`/ etc. —
/// `CT_TableStyleTextStyle` + `CT_TableStyleCellStyle` combined, see
/// [`crate::model::TableStylePart`]'s own doc comment), assuming its own `Start` was just consumed
/// by the caller (`wrapper_local_name` is that same element's local name) — consumes up to and
/// including its own closing tag. Only breaks on an `End` matching `wrapper_local_name` itself, not
/// just any `End` — `<a:tcTxStyle>`'s own closing tag can arrive as a "stray" event here
/// (`drawing::read_color` doesn't always consume it, depending on whether it found a color — see
/// that function's own doc comment), and must be safely ignored rather than mistaken for this
/// element's own end, the same pattern already used by [`parse_slide_background`] around
/// `drawing::read_fill`.
fn parse_table_style_part(
    reader: &mut Reader,
    wrapper_local_name: &[u8],
) -> Result<TableStylePart> {
    let mut part = TableStylePart::new();

    loop {
        match reader.read_event()? {
            Event::Empty(start) if start.local_name().as_ref() == b"tcTxStyle" => {
                part.bold = attribute_by_qname(&start, b"b").as_deref() == Some("on");
                part.italic = attribute_by_qname(&start, b"i").as_deref() == Some("on");
            }
            Event::Start(start) if start.local_name().as_ref() == b"tcTxStyle" => {
                part.bold = attribute_by_qname(&start, b"b").as_deref() == Some("on");
                part.italic = attribute_by_qname(&start, b"i").as_deref() == Some("on");
                part.text_color = drawing::read_color(reader)?;
            }

            Event::Start(start) if start.local_name().as_ref() == b"tcStyle" => {
                part.fill = parse_table_style_cell_fill(reader)?;
            }
            Event::Empty(start) if start.local_name().as_ref() == b"tcStyle" => {}

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            Event::End(end) if end.local_name().as_ref() == wrapper_local_name => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(part)
}

/// Parses `<a:tcStyle>`'s content looking for `<a:fill>`'s own `EG_FillProperties` choice, assuming
/// `<a:tcStyle>`'s own `Start` was just consumed by the caller — consumes up to and including
/// `<a:tcStyle>`'s own closing tag. `<a:tcBdr>`/`<a:cell3D>` are skipped (not modeled, see
/// `TableStylePart`'s own doc comment).
fn parse_table_style_cell_fill(reader: &mut Reader) -> Result<Option<Fill>> {
    let mut fill = None;

    loop {
        match reader.read_event()? {
            Event::Start(start) if start.local_name().as_ref() == b"fill" => {
                fill = drawing::read_fill(reader)?;
            }
            Event::Empty(start) if start.local_name().as_ref() == b"fill" => {}

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            Event::End(end) if end.local_name().as_ref() == b"tcStyle" => break,
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(fill)
}

/// Parses `ppt/theme/theme1.xml` (`CT_OfficeStyleSheet`) into a [`Theme`] —
/// `clrScheme`/`fontScheme` only, `fmtScheme` is always ignored (this crate never customizes it
/// either way, see [`Theme`]'s own doc comment).
fn parse_theme_xml(xml: &str) -> Result<Theme> {
    let mut reader = Reader::from_xml_str(xml);
    let mut name = String::new();
    let mut colors = ColorScheme::office_default();
    let mut fonts = FontScheme::office_default();

    loop {
        match reader.read_event()? {
            Event::Eof => break,

            Event::Start(start) if start.local_name().as_ref() == b"theme" => {
                if let Some(value) = attribute_by_qname(&start, b"name") {
                    name = value;
                }
            }
            Event::Start(start) if start.local_name().as_ref() == b"clrScheme" => {
                colors = parse_color_scheme(&mut reader)?;
            }
            Event::Start(start) if start.local_name().as_ref() == b"fontScheme" => {
                fonts = parse_font_scheme(&mut reader)?;
            }

            _ => {}
        }
    }

    Ok(Theme {
        name,
        colors,
        fonts,
    })
}

/// Parses `<a:clrScheme>`'s 12 color slots (a reader positioned right after its own opening tag was
/// consumed), stopping at `</a:clrScheme>`. Each slot's one child is either `<a:srgbClr val="..">`
/// (used directly) or `<a:sysClr val=".." lastClr="..">` (its `lastClr` fallback RGB is used
/// instead — mirrors `word_ooxml::ColorScheme`'s own reader, see that type's doc comment for why
/// the live system-color binding itself isn't preserved).
fn parse_color_scheme(reader: &mut Reader) -> Result<ColorScheme> {
    let mut colors = ColorScheme::office_default();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"clrScheme" => break,

            Event::Start(start) => {
                let slot = start.local_name().as_ref().to_vec();
                if let Some(value) = read_color_slot_value(reader, &slot)? {
                    assign_color_slot(&mut colors, &slot, value);
                }
            }

            _ => {}
        }
    }

    Ok(colors)
}

/// Reads a single color slot's value (`<a:dk1>`, `<a:accent1>`..), a reader positioned right after
/// that wrapper's own opening tag was consumed. Consumes up to and including the matching end tag
/// for `wrapper_local_name`.
fn read_color_slot_value(reader: &mut Reader, wrapper_local_name: &[u8]) -> Result<Option<String>> {
    let mut value = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == wrapper_local_name => break,

            Event::Empty(start) | Event::Start(start) => match start.local_name().as_ref() {
                b"srgbClr" => value = attribute_by_qname(&start, b"val"),
                b"sysClr" => {
                    value = attribute_by_qname(&start, b"lastClr")
                        .or_else(|| attribute_by_qname(&start, b"val"))
                }
                _ => {}
            },

            _ => {}
        }
    }

    Ok(value)
}

/// Assigns a parsed color value to the matching field of `colors`, by its `<a:clrScheme>` child
/// element name.
fn assign_color_slot(colors: &mut ColorScheme, slot: &[u8], value: String) {
    match slot {
        b"dk1" => colors.dark1 = value,
        b"lt1" => colors.light1 = value,
        b"dk2" => colors.dark2 = value,
        b"lt2" => colors.light2 = value,
        b"accent1" => colors.accent1 = value,
        b"accent2" => colors.accent2 = value,
        b"accent3" => colors.accent3 = value,
        b"accent4" => colors.accent4 = value,
        b"accent5" => colors.accent5 = value,
        b"accent6" => colors.accent6 = value,
        b"hlink" => colors.hyperlink = value,
        b"folHlink" => colors.followed_hyperlink = value,
        _ => {}
    }
}

/// Parses `<a:fontScheme>`'s `majorFont`/`minorFont` (a reader positioned right after its own
/// opening tag was consumed), stopping at `</a:fontScheme>`. Only each font collection's
/// `<a:latin>` typeface is captured, mirrors `word_ooxml::FontScheme`'s own reader.
fn parse_font_scheme(reader: &mut Reader) -> Result<FontScheme> {
    let mut fonts = FontScheme::office_default();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"fontScheme" => break,

            Event::Start(start) if start.local_name().as_ref() == b"majorFont" => {
                if let Some(typeface) = parse_font_collection_latin(reader, b"majorFont")? {
                    fonts.major_latin = typeface;
                }
            }
            Event::Start(start) if start.local_name().as_ref() == b"minorFont" => {
                if let Some(typeface) = parse_font_collection_latin(reader, b"minorFont")? {
                    fonts.minor_latin = typeface;
                }
            }

            _ => {}
        }
    }

    Ok(fonts)
}

/// Reads a `<a:majorFont>`/`<a:minorFont>` font collection's `<a:latin>` typeface, ignoring
/// `ea`/`cs`/per-script fallback children. Consumes up to and including the matching end tag for
/// `wrapper_local_name`.
fn parse_font_collection_latin(
    reader: &mut Reader,
    wrapper_local_name: &[u8],
) -> Result<Option<String>> {
    let mut typeface = None;

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == wrapper_local_name => break,

            Event::Empty(start) | Event::Start(start)
                if start.local_name().as_ref() == b"latin" =>
            {
                typeface = attribute_by_qname(&start, b"typeface");
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(typeface)
}

/// Parses `ppt/commentAuthors.xml` (`CT_CommentAuthorList`) into an `authorId -> (name, initials)`
/// map. **Not grounded on a real fixture** — see `model.rs`'s top-level doc comment.
fn parse_comment_authors_xml(xml: &str) -> Result<HashMap<u32, (String, String)>> {
    let mut authors = HashMap::new();
    let mut reader = Reader::from_xml_str(xml);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Empty(start) | Event::Start(start)
                if start.local_name().as_ref() == b"cmAuthor" =>
            {
                if let Some(id) =
                    attribute_by_qname(&start, b"id").and_then(|value| value.parse().ok())
                {
                    let name = attribute_by_qname(&start, b"name").unwrap_or_default();
                    let initials = attribute_by_qname(&start, b"initials").unwrap_or_default();
                    authors.insert(id, (name, initials));
                }
            }
            _ => {}
        }
    }

    Ok(authors)
}

/// Parses one `ppt/comments/commentN.xml` (`CT_CommentList`) into this slide's [`SlideComment`]s,
/// resolving each `<p:cm authorId="..">` against `authors` (an unresolvable `authorId` — no
/// matching entry in `ppt/commentAuthors.xml` — falls back to an empty author name/initials rather
/// than erroring, same forgiving posture as the rest of this reader). **Not grounded on a real
/// fixture** — see `model.rs`'s top-level doc comment.
fn parse_comments_xml(
    xml: &str,
    authors: &HashMap<u32, (String, String)>,
) -> Result<Vec<SlideComment>> {
    let mut comments = Vec::new();
    let mut reader = Reader::from_xml_str(xml);

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(start) if start.local_name().as_ref() == b"cm" => {
                let author_id: u32 = attribute_by_qname(&start, b"authorId")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let (author, initials) = authors.get(&author_id).cloned().unwrap_or_default();
                let date = attribute_by_qname(&start, b"dt").unwrap_or_default();
                comments.push(parse_comment_body(&mut reader, author, initials, date)?);
            }
            _ => {}
        }
    }

    Ok(comments)
}

/// Parses one `<p:cm>`'s own `<p:pos>`/`<p:text>` content, a reader positioned right after its own
/// opening tag was consumed. Stops at `</p:cm>`.
fn parse_comment_body(
    reader: &mut Reader,
    author: String,
    initials: String,
    date: String,
) -> Result<SlideComment> {
    let mut position_emu = (0i64, 0i64);
    let mut text = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::End(end) if end.local_name().as_ref() == b"cm" => break,

            Event::Empty(start) if start.local_name().as_ref() == b"pos" => {
                position_emu = read_xy_attributes(&start, b"x", b"y");
            }
            Event::Start(start) if start.local_name().as_ref() == b"text" => {
                text = read_element_text(reader)?;
            }

            Event::Start(_) => skip_subtree(reader)?,
            Event::Empty(_) => {}
            _ => {}
        }
    }

    Ok(SlideComment {
        author,
        initials,
        date,
        position_emu,
        text,
    })
}

/// Reads plain text content up to the next `Event::End` (a reader positioned right after a leaf
/// text element's own opening tag was consumed, e.g. `<p:text>`) — decodes
/// `Event::Text`/`Event::GeneralRef` the same way run text is decoded elsewhere in this workspace
/// (`xml_core::decode_text`/`decode_general_ref`), so a comment containing `&`/`<`/`>`/a numeric
/// character reference round-trips correctly.
fn read_element_text(reader: &mut Reader) -> Result<String> {
    let mut text = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof | Event::End(_) => break,
            Event::Text(bytes_text) => {
                text.push_str(&xml_core::decode_text(&bytes_text)?);
            }
            Event::GeneralRef(bytes_ref) => {
                if let Some(decoded) = xml_core::decode_general_ref(&bytes_ref)? {
                    text.push_str(&decoded);
                }
            }
            _ => {}
        }
    }

    Ok(text)
}

/// Reads the decoded value of an attribute matched by its full qualified name (including any
/// namespace prefix, e.g. `b"r:id"`), not just its local name — unlike `opc::relationship`'s own
/// attribute lookup, this matters here because a single element (`<p:sldId id="256" r:id="rId2"/>`)
/// can carry both an unprefixed `id` and a prefixed `r:id` attribute, which share the same *local*
/// name and would otherwise be ambiguous.
fn attribute_by_qname(start: &BytesStart, qname: &[u8]) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == qname)
        .and_then(|attribute| {
            xml_core::decode_attribute_value(attribute.value.as_ref())
                .ok()
                .flatten()
        })
}

/// Skips an already-opened element's entire subtree, including its own closing tag — same helper
/// `drawing::reader` keeps privately for the same purpose (gracefully ignoring an unmodeled child
/// element rather than failing the whole document).
fn skip_subtree(reader: &mut Reader) -> Result<()> {
    let mut depth: u32 = 0;
    loop {
        match reader.read_event()? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                if depth == 0 {
                    return Ok(());
                }
                depth -= 1;
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
