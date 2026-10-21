//! Writing our [`Presentation`] model out to a valid `.pptx` package.
//!
//! Real PowerPoint packages always carry a slide master, at least one slide layout, and a theme,
//! even for the simplest deck — unlike Word/Excel, which can get away with a much smaller minimum
//! part set. This crate has no customizable model for the slide master/layout, so `write_to` always
//! emits one fixed, minimal master/layout pair alongside the slides the caller actually populated —
//! the theme, though, follows [`Presentation::theme`], still always written (mandatory per
//! ECMA-376), just with caller-supplied content when set. A notes master/notes slide pair is
//! emitted the same way as the master/layout, but only when
//! at least one slide actually has notes (see [`Slide::notes`]'s doc comment) — genuinely optional,
//! unlike the master/layout/theme triplet. Per-slide comments (`ppt/comments/`,
//! `ppt/commentAuthors.xml`) follow the same "only when actually present" posture as notes.

use std::io::{Seek, Write};

use drawing::{ShapeProperties, TextAnchor};
use opc::{Package, Part, Relationship, Relationships, TargetMode};
use xml_core::{BytesDecl, BytesEnd, BytesStart, Event, Writer};

use crate::error::Result;
use crate::model::{
    AutoShape, ColorScheme, Connector, CustomPropertyValue, DEFAULT_NOTES_HEIGHT_EMU,
    DEFAULT_NOTES_WIDTH_EMU, DocumentProperties, FontScheme, Placeholder, PlaceholderKind,
    Presentation, Shape, ShapeGroup, Slide, SlideComment, SlideHyperlinkTarget, SlideMedia,
    SlideTable, SlideTableStyle, TableCell, TableCellProperties, TableRow, TableStylePart, Theme,
};

const P_NAMESPACE: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R_NAMESPACE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CHART_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const TABLE_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
const DIAGRAM_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";

/// Validates a table style id against `ST_Guid`
/// (`\{[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}\}`, confirmed against the
/// ECMA-376/ISO-IEC 29500-4 `dml-main.xsd` schema — both `CT_TableStyle@styleId` and
/// `<a:tableStyleId>`'s content are typed `s:ST_Guid`). Called before writing either, so a
/// caller-supplied id like `"CustomTableStyle"` fails loudly here instead of silently producing a
/// `.pptx` PowerPoint reports as damaged.
fn validate_table_style_id(id: &str) -> Result<()> {
    let bytes = id.as_bytes();
    let is_hex_run = |s: &[u8], len: usize| s.len() == len && s.iter().all(u8::is_ascii_hexdigit);
    let valid = bytes.len() == 38
        && bytes[0] == b'{'
        && bytes[37] == b'}'
        && bytes[9] == b'-'
        && bytes[14] == b'-'
        && bytes[19] == b'-'
        && bytes[24] == b'-'
        && is_hex_run(&bytes[1..9], 8)
        && is_hex_run(&bytes[10..14], 4)
        && is_hex_run(&bytes[15..19], 4)
        && is_hex_run(&bytes[20..24], 4)
        && is_hex_run(&bytes[25..37], 12)
        // ST_Guid's pattern is `[0-9A-F]`, not `[0-9A-Fa-f]` — real Office output is always
        // uppercase, so reject lowercase too rather than silently accepting XML a strict validator
        // would still flag.
        && id.chars().all(|c| !c.is_ascii_hexdigit() || c.is_ascii_digit() || c.is_ascii_uppercase());
    if valid {
        Ok(())
    } else {
        Err(crate::error::Error::InvalidTableStyleId(id.to_string()))
    }
}

const OFFICE_DOCUMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const SLIDE_MASTER_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const SLIDE_LAYOUT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const SLIDE_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const THEME_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const PRES_PROPS_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
const VIEW_PROPS_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps";
const TABLE_STYLES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
const CORE_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const EXTENDED_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const IMAGE_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const CHART_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
const NOTES_MASTER_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const NOTES_SLIDE_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
// Legacy (`CT_Comment`, ECMA-376 §19.4) presentation comments — not grounded on a real fixture, see
// `model.rs`'s top-level doc comment and `SlideComment`'s own doc comment.
const COMMENT_AUTHORS_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/commentAuthors";
const COMMENT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const HYPERLINK_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const VIDEO_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video";
const AUDIO_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/audio";
const FONT_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font";
const CUSTOM_PROPERTIES_RELATIONSHIP_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";

const PRESENTATION_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const SLIDE_MASTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const SLIDE_LAYOUT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const THEME_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const PRES_PROPS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";
const VIEW_PROPS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml";
const TABLE_STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
const CORE_PROPERTIES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-package.core-properties+xml";
const EXTENDED_PROPERTIES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const NOTES_MASTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const NOTES_SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const COMMENT_AUTHORS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml";
const COMMENT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.comments+xml";
const FONT_CONTENT_TYPE: &str = "application/x-fontdata";
const CUSTOM_PROPERTIES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const CUSTOM_PROPERTIES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const CUSTOM_PROPERTIES_VT_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
// The fixed custom-properties `fmtid` (`CT_Property`'s own attribute) every OOXML application uses,
// and the first `pid` real Office assigns a custom property (`0`/`1` are reserved by the format) —
// same values `excel-ooxml`/`word-ooxml` already use for their own custom properties.
const CUSTOM_PROPERTY_FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";
const FIRST_CUSTOM_PROPERTY_PID: i32 = 2;
// The "No Style, No Grid" built-in table style GUID (part of the OOXML specification's own
// reference material, not implementation-specific) — confirmed as a genuine value real PowerPoint
// writes for `<a:tableStyleId>` via a real fixture, and already used unchanged as
// `to_table_styles_xml`'s own `def` attribute.
const TABLE_STYLE_GUID: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

const PRESENTATION_PART_NAME: &str = "/ppt/presentation.xml";
const PRESENTATION_ENTRY_NAME: &str = "ppt/presentation.xml";
const SLIDE_MASTER_PART_NAME: &str = "/ppt/slideMasters/slideMaster1.xml";
const SLIDE_MASTER_ENTRY_NAME: &str = "slideMasters/slideMaster1.xml";
const SLIDE_LAYOUT_PART_NAME: &str = "/ppt/slideLayouts/slideLayout1.xml";
const THEME_PART_NAME: &str = "/ppt/theme/theme1.xml";
const THEME_ENTRY_NAME: &str = "theme/theme1.xml";
// The notes master gets its own dedicated theme part (`theme2.xml`) rather than sharing
// `theme1.xml` with the slide master. Real PowerPoint files always give the notes master its own
// theme relationship (confirmed via a real multi-master fixture, and via PowerPoint's own repair of
// a file that shared theme1.xml between slideMaster and notesMaster: repair split them apart by
// synthesizing a brand-new theme2.xml and repointing notesMaster1.xml's theme relationship at it).
// Sharing the same theme part between the two is apparently what real PowerPoint's loader rejects
// as corrupt, even though nothing in the schema text forbids two relationships targeting the same
// part.
const NOTES_MASTER_THEME_PART_NAME: &str = "/ppt/theme/theme2.xml";
const NOTES_MASTER_THEME_ENTRY_NAME: &str = "../theme/theme2.xml";
const PRES_PROPS_PART_NAME: &str = "/ppt/presProps.xml";
const PRES_PROPS_ENTRY_NAME: &str = "presProps.xml";
const VIEW_PROPS_PART_NAME: &str = "/ppt/viewProps.xml";
const VIEW_PROPS_ENTRY_NAME: &str = "viewProps.xml";
const TABLE_STYLES_PART_NAME: &str = "/ppt/tableStyles.xml";
const TABLE_STYLES_ENTRY_NAME: &str = "tableStyles.xml";
const CORE_PROPERTIES_PART_NAME: &str = "/docProps/core.xml";
const CORE_PROPERTIES_ENTRY_NAME: &str = "docProps/core.xml";
const EXTENDED_PROPERTIES_PART_NAME: &str = "/docProps/app.xml";
const EXTENDED_PROPERTIES_ENTRY_NAME: &str = "docProps/app.xml";
const NOTES_MASTER_PART_NAME: &str = "/ppt/notesMasters/notesMaster1.xml";
const NOTES_MASTER_ENTRY_NAME: &str = "notesMasters/notesMaster1.xml";
const COMMENT_AUTHORS_PART_NAME: &str = "/ppt/commentAuthors.xml";
const COMMENT_AUTHORS_ENTRY_NAME: &str = "commentAuthors.xml";
const CUSTOM_PROPERTIES_PART_NAME: &str = "/docProps/custom.xml";
const CUSTOM_PROPERTIES_ENTRY_NAME: &str = "docProps/custom.xml";

// The numeric id PowerPoint expects the (single, fixed) slide master to have in
// `<p:sldMasterIdLst>` — real PowerPoint starts slide master ids at this exact value.
const SLIDE_MASTER_ID: u32 = 2_147_483_648;
// Real slide ids start at 256; not otherwise meaningful, just needs to be unique per `<p:sldId>`.
const FIRST_SLIDE_ID: u32 = 256;

impl Presentation {
    /// Writes this presentation to a seekable byte sink as a valid `.pptx` package:
    /// `[Content_Types].xml`, `_rels/.rels`, `ppt/presentation.xml` (+ rels), one
    /// `ppt/slides/slideN.xml` (+ rels) per slide, a single fixed
    /// `ppt/slideMasters/slideMaster1.xml` (+ rels), `ppt/slideLayouts/slideLayout1.xml` (+ rels),
    /// `ppt/theme/theme1.xml`, `ppt/presProps.xml`, `ppt/viewProps.xml`, `ppt/tableStyles.xml`,
    /// `docProps/core.xml`, `docProps/app.xml`, plus `ppt/media/imageN.*`/ `ppt/charts/chartN.xml`
    /// for any picture/chart shape, plus (only if at least one slide has notes) a single fixed
    /// `ppt/notesMasters/notesMaster1.xml` and one `ppt/notesSlides/notesSlideN.xml` per slide that
    /// has notes, plus (only if at least one slide has comments) a single `ppt/commentAuthors.xml`
    /// and one `ppt/comments/commentN.xml` per slide that has comments. `ppt/theme/theme1.xml`'s
    /// content follows `self.theme` (Office's own built-in default when unset) — see this module's
    /// own doc comment for why the slide master/layout pair itself stays fixed.
    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<W> {
        let mut package = Package::new();

        package.add_relationship(Relationship {
            id: "rId1".to_string(),
            rel_type: OFFICE_DOCUMENT_RELATIONSHIP_TYPE.to_string(),
            target: PRESENTATION_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        package.add_relationship(Relationship {
            id: "rId2".to_string(),
            rel_type: CORE_PROPERTIES_RELATIONSHIP_TYPE.to_string(),
            target: CORE_PROPERTIES_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        package.add_relationship(Relationship {
            id: "rId3".to_string(),
            rel_type: EXTENDED_PROPERTIES_RELATIONSHIP_TYPE.to_string(),
            target: EXTENDED_PROPERTIES_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        // `docProps/custom.xml`. Genuinely optional, same "only written when actually set"
        // convention as notes/comments, so its package-root relationship is only added when there's
        // actually a custom property to write.
        if !self.properties.custom_properties.is_empty() {
            package.add_relationship(Relationship {
                id: "rId4".to_string(),
                rel_type: CUSTOM_PROPERTIES_RELATIONSHIP_TYPE.to_string(),
                target: CUSTOM_PROPERTIES_ENTRY_NAME.to_string(),
                target_mode: TargetMode::Internal,
            });
        }

        // `ppt/presentation.xml`'s own relationships: the slide master first (rId1), then one
        // relationship per slide, then (if needed) the notes master, then
        // theme/presProps/viewProps/tableStyles — order doesn't matter to the schema, but mirrors
        // the real fixture's own numbering closely enough to stay easy to cross-check by hand.
        let mut presentation_relationships = Relationships::new();
        presentation_relationships.add(Relationship {
            id: "rId1".to_string(),
            rel_type: SLIDE_MASTER_RELATIONSHIP_TYPE.to_string(),
            target: SLIDE_MASTER_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });

        // Package-wide state, threaded through every slide: pictures/ charts encountered anywhere
        // are collected as package-wide-unique media/chart parts, added to the package once every
        // slide has been walked — mirrors `word-ooxml`/`excel-ooxml`'s own `PackageState`.
        let mut state = PackageState::new();
        let has_notes = self.slides.iter().any(|slide| slide.notes.is_some());

        let mut next_relationship_id = 2u32;
        let mut slide_ids: Vec<(u32, String)> = Vec::new();
        for (index, slide) in self.slides.iter().enumerate() {
            let slide_number = index + 1;
            let relationship_id = format!("rId{next_relationship_id}");
            next_relationship_id += 1;

            presentation_relationships.add(Relationship {
                id: relationship_id.clone(),
                rel_type: SLIDE_RELATIONSHIP_TYPE.to_string(),
                target: format!("slides/slide{slide_number}.xml"),
                target_mode: TargetMode::Internal,
            });
            slide_ids.push((FIRST_SLIDE_ID + index as u32, relationship_id));

            // Every slide reserves rId1 for its slide layout relationship; pictures/charts
            // encountered while writing the shape tree below get rId2 onward, assigned as they're
            // written.
            let mut slide_relationships = PartRelationships::new();
            slide_relationships.add(
                SLIDE_LAYOUT_RELATIONSHIP_TYPE,
                "../slideLayouts/slideLayout1.xml".to_string(),
            );

            let slide_xml = to_slide_xml(slide, &mut state, &mut slide_relationships)?;

            if let Some(notes) = &slide.notes {
                let notes_entry_name = format!("notesSlides/notesSlide{slide_number}.xml");
                slide_relationships.add(
                    NOTES_SLIDE_RELATIONSHIP_TYPE,
                    format!("../{notes_entry_name}"),
                );

                let mut notes_part_relationships = Relationships::new();
                notes_part_relationships.add(Relationship {
                    id: "rId1".to_string(),
                    rel_type: NOTES_MASTER_RELATIONSHIP_TYPE.to_string(),
                    target: "../notesMasters/notesMaster1.xml".to_string(),
                    target_mode: TargetMode::Internal,
                });
                notes_part_relationships.add(Relationship {
                    id: "rId2".to_string(),
                    rel_type: SLIDE_RELATIONSHIP_TYPE.to_string(),
                    target: format!("../slides/slide{slide_number}.xml"),
                    target_mode: TargetMode::Internal,
                });
                let mut notes_part = Part::new(
                    format!("/ppt/{notes_entry_name}"),
                    NOTES_SLIDE_CONTENT_TYPE,
                    to_notes_slide_xml(notes)?,
                );
                notes_part.relationships = notes_part_relationships;
                package.add_part(notes_part);
            }

            // A slide's comments live in their own part (`ppt/comments/commentN.xml`), reached via
            // a `comments`- typed relationship on the slide's own `.rels` — same "only written when
            // actually set" convention as notes. **Not grounded on a real fixture**, see
            // `model.rs`'s top-level doc comment and `SlideComment`'s own doc comment.
            if !slide.comments.is_empty() {
                let comments_entry_name = format!("comments/comment{slide_number}.xml");
                slide_relationships.add(
                    COMMENT_RELATIONSHIP_TYPE,
                    format!("../{comments_entry_name}"),
                );
                let comments_xml = to_comments_xml(&slide.comments, &mut state.comment_authors);
                package.add_part(Part::new(
                    format!("/ppt/{comments_entry_name}"),
                    COMMENT_CONTENT_TYPE,
                    comments_xml,
                ));
            }

            let mut slide_part = Part::new(
                format!("/ppt/slides/slide{slide_number}.xml"),
                SLIDE_CONTENT_TYPE,
                slide_xml,
            );
            slide_part.relationships = slide_relationships.relationships;
            package.add_part(slide_part);
        }

        // `ppt/commentAuthors.xml` is package-wide (every comment on every slide references one
        // shared author list by numeric id, see `state.comment_authors`'s own doc comment) —
        // written once, after every slide has been walked and every author it uses collected, same
        // "collect while walking slides, write once at the end" posture as
        // `state.media`/`state.charts`.
        if !state.comment_authors.authors.is_empty() {
            let relationship_id = format!("rId{next_relationship_id}");
            next_relationship_id += 1;
            presentation_relationships.add(Relationship {
                id: relationship_id,
                rel_type: COMMENT_AUTHORS_RELATIONSHIP_TYPE.to_string(),
                target: COMMENT_AUTHORS_ENTRY_NAME.to_string(),
                target_mode: TargetMode::Internal,
            });
            package.add_part(Part::new(
                COMMENT_AUTHORS_PART_NAME,
                COMMENT_AUTHORS_CONTENT_TYPE,
                to_comment_authors_xml(&state.comment_authors),
            ));
        }

        let notes_master_relationship_id = if has_notes {
            let relationship_id = format!("rId{next_relationship_id}");
            next_relationship_id += 1;
            presentation_relationships.add(Relationship {
                id: relationship_id.clone(),
                rel_type: NOTES_MASTER_RELATIONSHIP_TYPE.to_string(),
                target: NOTES_MASTER_ENTRY_NAME.to_string(),
                target_mode: TargetMode::Internal,
            });
            Some(relationship_id)
        } else {
            None
        };

        let theme_relationship_id = format!("rId{next_relationship_id}");
        next_relationship_id += 1;
        presentation_relationships.add(Relationship {
            id: theme_relationship_id,
            rel_type: THEME_RELATIONSHIP_TYPE.to_string(),
            target: THEME_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        let pres_props_relationship_id = format!("rId{next_relationship_id}");
        next_relationship_id += 1;
        presentation_relationships.add(Relationship {
            id: pres_props_relationship_id,
            rel_type: PRES_PROPS_RELATIONSHIP_TYPE.to_string(),
            target: PRES_PROPS_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        let view_props_relationship_id = format!("rId{next_relationship_id}");
        next_relationship_id += 1;
        presentation_relationships.add(Relationship {
            id: view_props_relationship_id,
            rel_type: VIEW_PROPS_RELATIONSHIP_TYPE.to_string(),
            target: VIEW_PROPS_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });
        let table_styles_relationship_id = format!("rId{next_relationship_id}");
        next_relationship_id += 1;
        presentation_relationships.add(Relationship {
            id: table_styles_relationship_id,
            rel_type: TABLE_STYLES_RELATIONSHIP_TYPE.to_string(),
            target: TABLE_STYLES_ENTRY_NAME.to_string(),
            target_mode: TargetMode::Internal,
        });

        // Embedded fonts — each font's own variant (regular/bold/italic/boldItalic) present becomes
        // its own `ppt/fonts/fontN.fntdata` part plus a presentation-level `font`- typed
        // relationship, referenced by `<p:embeddedFont>`'s own `r:id` attributes (resolved up front
        // here, since `to_presentation_xml` itself has no relationship accumulator of its own to
        // register against).
        let mut next_font_index = 1u32;
        let mut resolved_fonts: Vec<ResolvedEmbeddedFont> = Vec::new();
        for font in &self.embedded_fonts {
            resolved_fonts.push(ResolvedEmbeddedFont {
                typeface: font.typeface.clone(),
                regular: register_embedded_font_variant(
                    &mut package,
                    &mut presentation_relationships,
                    &mut next_relationship_id,
                    &mut next_font_index,
                    &font.regular,
                ),
                bold: register_embedded_font_variant(
                    &mut package,
                    &mut presentation_relationships,
                    &mut next_relationship_id,
                    &mut next_font_index,
                    &font.bold,
                ),
                italic: register_embedded_font_variant(
                    &mut package,
                    &mut presentation_relationships,
                    &mut next_relationship_id,
                    &mut next_font_index,
                    &font.italic,
                ),
                bold_italic: register_embedded_font_variant(
                    &mut package,
                    &mut presentation_relationships,
                    &mut next_relationship_id,
                    &mut next_font_index,
                    &font.bold_italic,
                ),
            });
        }

        let mut presentation_part = Part::new(
            PRESENTATION_PART_NAME,
            PRESENTATION_CONTENT_TYPE,
            to_presentation_xml(
                self,
                &slide_ids,
                notes_master_relationship_id.as_deref(),
                &resolved_fonts,
            )?,
        );
        presentation_part.relationships = presentation_relationships;
        package.add_part(presentation_part);

        let mut slide_master_part = Part::new(
            SLIDE_MASTER_PART_NAME,
            SLIDE_MASTER_CONTENT_TYPE,
            to_slide_master_xml()?,
        );
        slide_master_part.relationships.add(Relationship {
            id: "rId1".to_string(),
            rel_type: SLIDE_LAYOUT_RELATIONSHIP_TYPE.to_string(),
            target: "../slideLayouts/slideLayout1.xml".to_string(),
            target_mode: TargetMode::Internal,
        });
        slide_master_part.relationships.add(Relationship {
            id: "rId2".to_string(),
            rel_type: THEME_RELATIONSHIP_TYPE.to_string(),
            target: "../theme/theme1.xml".to_string(),
            target_mode: TargetMode::Internal,
        });
        package.add_part(slide_master_part);

        let mut slide_layout_part = Part::new(
            SLIDE_LAYOUT_PART_NAME,
            SLIDE_LAYOUT_CONTENT_TYPE,
            to_slide_layout_xml()?,
        );
        slide_layout_part.relationships.add(Relationship {
            id: "rId1".to_string(),
            rel_type: SLIDE_MASTER_RELATIONSHIP_TYPE.to_string(),
            target: "../slideMasters/slideMaster1.xml".to_string(),
            target_mode: TargetMode::Internal,
        });
        package.add_part(slide_layout_part);

        package.add_part(Part::new(
            THEME_PART_NAME,
            THEME_CONTENT_TYPE,
            to_theme_xml(self.theme.as_ref()),
        ));
        package.add_part(Part::new(
            PRES_PROPS_PART_NAME,
            PRES_PROPS_CONTENT_TYPE,
            to_pres_props_xml(),
        ));
        package.add_part(Part::new(
            VIEW_PROPS_PART_NAME,
            VIEW_PROPS_CONTENT_TYPE,
            to_view_props_xml(),
        ));
        package.add_part(Part::new(
            TABLE_STYLES_PART_NAME,
            TABLE_STYLES_CONTENT_TYPE,
            to_table_styles_xml(&self.table_styles)?,
        ));

        if has_notes {
            let mut notes_master_part = Part::new(
                NOTES_MASTER_PART_NAME,
                NOTES_MASTER_CONTENT_TYPE,
                to_notes_master_xml()?,
            );
            notes_master_part.relationships.add(Relationship {
                id: "rId1".to_string(),
                rel_type: THEME_RELATIONSHIP_TYPE.to_string(),
                target: NOTES_MASTER_THEME_ENTRY_NAME.to_string(),
                target_mode: TargetMode::Internal,
            });
            package.add_part(notes_master_part);
            // Dedicated theme part for the notes master (see `NOTES_MASTER_THEME_PART_NAME`'s doc
            // comment for why this can't just reuse `theme1.xml`). Content is the same fixed theme
            // as `theme1.xml`; only the part identity needs to differ.
            package.add_part(Part::new(
                NOTES_MASTER_THEME_PART_NAME,
                THEME_CONTENT_TYPE,
                to_theme_xml(None),
            ));
        }

        for media_part in state.media.parts {
            package.add_part(Part::new(
                format!("/ppt/{}", media_part.entry_name),
                media_part.content_type,
                media_part.data,
            ));
        }
        for chart_part in state.charts.parts {
            package.add_part(Part::new(
                format!("/ppt/{}", chart_part.entry_name),
                CHART_CONTENT_TYPE,
                chart_part.data,
            ));
        }

        package.add_part(Part::new(
            CORE_PROPERTIES_PART_NAME,
            CORE_PROPERTIES_CONTENT_TYPE,
            to_core_properties_xml(&self.properties)?,
        ));
        package.add_part(Part::new(
            EXTENDED_PROPERTIES_PART_NAME,
            EXTENDED_PROPERTIES_CONTENT_TYPE,
            to_extended_properties_xml(self.slides.len(), &self.properties)?,
        ));
        // `docProps/custom.xml`. Only written when there's actually a custom property (its
        // package-root relationship above is conditional on the same check).
        if !self.properties.custom_properties.is_empty() {
            package.add_part(Part::new(
                CUSTOM_PROPERTIES_PART_NAME,
                CUSTOM_PROPERTIES_CONTENT_TYPE,
                to_custom_properties_xml(&self.properties.custom_properties)?,
            ));
        }

        Ok(package.write_to(writer)?)
    }
}

/// One [`EmbeddedFont`]'s variants, resolved to already-registered relationship ids — built by
/// `write_to` before calling `to_presentation_xml` (which has no relationship accumulator of its
/// own to register a font part against).
struct ResolvedEmbeddedFont {
    typeface: String,
    regular: Option<String>,
    bold: Option<String>,
    italic: Option<String>,
    bold_italic: Option<String>,
}

/// Registers one embedded-font variant's bytes, if present: a new `ppt/fonts/fontN.fntdata` part
/// (package-wide-unique index, shared across every font/variant) plus a `font`-typed relationship
/// on `ppt/presentation.xml`'s own relationships, returning that relationship's id. Returns `None`
/// without touching either accumulator when `data` itself is `None` (no part/relationship for a
/// variant that was never set).
fn register_embedded_font_variant(
    package: &mut Package,
    presentation_relationships: &mut Relationships,
    next_relationship_id: &mut u32,
    next_font_index: &mut u32,
    data: &Option<Vec<u8>>,
) -> Option<String> {
    let data = data.as_ref()?;
    let entry_name = format!("fonts/font{next_font_index}.fntdata");
    *next_font_index += 1;
    let relationship_id = format!("rId{next_relationship_id}");
    *next_relationship_id += 1;
    presentation_relationships.add(Relationship {
        id: relationship_id.clone(),
        rel_type: FONT_RELATIONSHIP_TYPE.to_string(),
        target: entry_name.clone(),
        target_mode: TargetMode::Internal,
    });
    package.add_part(Part::new(
        format!("/ppt/{entry_name}"),
        FONT_CONTENT_TYPE,
        data.clone(),
    ));
    Some(relationship_id)
}

/// Package-wide state threaded through every slide while writing: pictures ([`MediaRegistry`]) and
/// charts ([`ChartRegistry`]) encountered on any slide. Mirrors `word-ooxml`/`excel-ooxml`'s own
/// `PackageState` — media/ chart part names must be unique across the whole package, not just
/// within one slide.
struct PackageState {
    media: MediaRegistry,
    charts: ChartRegistry,
    comment_authors: CommentAuthorRegistry,
}

impl PackageState {
    fn new() -> Self {
        Self {
            media: MediaRegistry::new(),
            charts: ChartRegistry::new(),
            comment_authors: CommentAuthorRegistry::new(),
        }
    }
}

/// The package-wide, deduplicated list of comment authors every [`SlideComment`] across every slide
/// references by numeric id (`<p:cm authorId="..">`) — real PowerPoint avoids a duplicate
/// `<p:cmAuthor>` entry for the same author appearing on several comments, so this crate does too,
/// matching each comment's `author`/`initials` against what's already registered (by name) before
/// adding a new entry.
struct CommentAuthorRegistry {
    authors: Vec<(String, String)>,
}

impl CommentAuthorRegistry {
    fn new() -> Self {
        Self {
            authors: Vec::new(),
        }
    }

    /// Returns this author's numeric id, registering a new `(name, initials)` entry (id = its
    /// index) the first time this name is seen.
    fn id_for(&mut self, name: &str, initials: &str) -> u32 {
        if let Some(index) = self
            .authors
            .iter()
            .position(|(existing_name, _)| existing_name == name)
        {
            return index as u32;
        }
        let id = self.authors.len() as u32;
        self.authors.push((name.to_string(), initials.to_string()));
        id
    }
}

struct MediaRegistry {
    next_index: usize,
    parts: Vec<MediaPart>,
}

struct MediaPart {
    /// Relative to `ppt/`, e.g. `media/image1.png`.
    entry_name: String,
    content_type: &'static str,
    data: Vec<u8>,
}

impl MediaRegistry {
    fn new() -> Self {
        Self {
            next_index: 1,
            parts: Vec::new(),
        }
    }

    fn register(&mut self, picture: &crate::model::Picture) -> String {
        self.register_bytes(
            "image",
            &picture.data,
            picture.format.extension(),
            picture.format.content_type(),
        )
    }

    /// Registers any package-wide-unique `ppt/media/*` part — generalizes
    /// [`MediaRegistry::register`] (pictures) for [`SlideMedia`]'s own video/audio bytes and
    /// poster-frame image, which don't have a [`crate::model::Picture`] to read a format off of.
    /// `base_name` picks the part's own file-name prefix (`"image"` for
    /// [`MediaRegistry::register`]'s own pictures, continuing this crate's earlier `image{N}`
    /// convention unchanged; `"media"` for video/audio clips) — cosmetic only, real PowerPoint
    /// itself is inconsistent about it across media kinds, but keeping pictures on their existing
    /// convention avoids any risk of colliding with `image{N}` indices a caller might already be
    /// relying on indirectly (e.g. inspecting a generated file by hand).
    fn register_bytes(
        &mut self,
        base_name: &str,
        data: &[u8],
        extension: &str,
        content_type: &'static str,
    ) -> String {
        let index = self.next_index;
        self.next_index += 1;
        let entry_name = format!("media/{base_name}{index}.{extension}");
        self.parts.push(MediaPart {
            entry_name: entry_name.clone(),
            content_type,
            data: data.to_vec(),
        });
        entry_name
    }
}

struct ChartRegistry {
    next_index: usize,
    parts: Vec<ChartPart>,
}

struct ChartPart {
    /// Relative to `ppt/`, e.g. `charts/chart1.xml`.
    entry_name: String,
    data: Vec<u8>,
}

impl ChartRegistry {
    fn new() -> Self {
        Self {
            next_index: 1,
            parts: Vec::new(),
        }
    }

    fn register(&mut self, chart_space: &chart::ChartSpace) -> Result<String> {
        let index = self.next_index;
        self.next_index += 1;
        let entry_name = format!("charts/chart{index}.xml");
        let mut chart_writer = Writer::new(Vec::new());
        chart::write_chart_space(&mut chart_writer, chart_space)?;
        self.parts.push(ChartPart {
            entry_name: entry_name.clone(),
            data: chart_writer.into_inner(),
        });
        Ok(entry_name)
    }
}

/// The relationships being accumulated for a single slide (or notes slide) part while it's being
/// written — each part has its own independently- scoped `rIdN` numbering. Mirrors `word-ooxml`'s
/// own `PartRelationships`.
struct PartRelationships {
    next_id: usize,
    relationships: Relationships,
}

impl PartRelationships {
    fn new() -> Self {
        Self {
            next_id: 1,
            relationships: Relationships::new(),
        }
    }

    fn add(&mut self, rel_type: &str, target: impl Into<String>) -> String {
        let id = format!("rId{}", self.next_id);
        self.next_id += 1;
        self.relationships.add(Relationship {
            id: id.clone(),
            rel_type: rel_type.to_string(),
            target: target.into(),
            target_mode: TargetMode::Internal,
        });
        id
    }

    /// Same as [`PartRelationships::add`], but for a relationship pointing outside the package
    /// (`TargetMode::External`) — e.g. a hyperlink's own target URL.
    fn add_external(&mut self, rel_type: &str, target: impl Into<String>) -> String {
        let id = format!("rId{}", self.next_id);
        self.next_id += 1;
        self.relationships.add(Relationship {
            id: id.clone(),
            rel_type: rel_type.to_string(),
            target: target.into(),
            target_mode: TargetMode::External,
        });
        id
    }
}

/// Serializes `ppt/presentation.xml` (`CT_Presentation`).
fn to_presentation_xml(
    presentation: &Presentation,
    slide_ids: &[(u32, String)],
    notes_master_relationship_id: Option<&str>,
    embedded_fonts: &[ResolvedEmbeddedFont],
) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:presentation");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    // `embedTrueTypeFonts="1"` tells real PowerPoint the deck carries its own embedded fonts — only
    // set when it actually does, same "only write what was actually set" convention as everywhere
    // else in this crate.
    if !embedded_fonts.is_empty() {
        root.push_attribute(("embedTrueTypeFonts", "1"));
    }
    writer.write_event(Event::Start(root))?;

    writer.write_event(Event::Start(BytesStart::new("p:sldMasterIdLst")))?;
    let mut master_id = BytesStart::new("p:sldMasterId");
    master_id.push_attribute(("id", SLIDE_MASTER_ID.to_string().as_str()));
    master_id.push_attribute(("r:id", "rId1"));
    writer.write_event(Event::Empty(master_id))?;
    writer.write_event(Event::End(BytesEnd::new("p:sldMasterIdLst")))?;

    // `<p:notesMasterIdLst>` sits right after `<p:sldMasterIdLst>` in `CT_Presentation`'s own child
    // sequence, before `<p:sldIdLst>` — only written when a notes master actually exists (see
    // `write_to`). Unlike `<p:sldMasterId>`/`<p:sldLayoutId>`, `<p:notesMasterId>` has no `id`
    // attribute at all, only `r:id` (confirmed against a real notes-bearing `.pptx` fixture).
    if let Some(relationship_id) = notes_master_relationship_id {
        writer.write_event(Event::Start(BytesStart::new("p:notesMasterIdLst")))?;
        let mut notes_master_id = BytesStart::new("p:notesMasterId");
        notes_master_id.push_attribute(("r:id", relationship_id));
        writer.write_event(Event::Empty(notes_master_id))?;
        writer.write_event(Event::End(BytesEnd::new("p:notesMasterIdLst")))?;
    }

    writer.write_event(Event::Start(BytesStart::new("p:sldIdLst")))?;
    for (numeric_id, relationship_id) in slide_ids {
        let mut slide_id = BytesStart::new("p:sldId");
        slide_id.push_attribute(("id", numeric_id.to_string().as_str()));
        slide_id.push_attribute(("r:id", relationship_id.as_str()));
        writer.write_event(Event::Empty(slide_id))?;
    }
    writer.write_event(Event::End(BytesEnd::new("p:sldIdLst")))?;

    let mut slide_size = BytesStart::new("p:sldSz");
    slide_size.push_attribute(("cx", presentation.slide_width_emu.to_string().as_str()));
    slide_size.push_attribute(("cy", presentation.slide_height_emu.to_string().as_str()));
    writer.write_event(Event::Empty(slide_size))?;

    let mut notes_size = BytesStart::new("p:notesSz");
    notes_size.push_attribute(("cx", DEFAULT_NOTES_WIDTH_EMU.to_string().as_str()));
    notes_size.push_attribute(("cy", DEFAULT_NOTES_HEIGHT_EMU.to_string().as_str()));
    writer.write_event(Event::Empty(notes_size))?;

    // `<p:embeddedFontLst>`. Sits after `<p:notesSz>` in `CT_Presentation`'s own child sequence
    // (before `<p:custShowLst>`/`<p:photoAlbum>`/etc., none of which this crate models); only
    // written when at least one font was actually embedded, same "only write what was actually set"
    // convention as everywhere else in this crate. **Not grounded on a real fixture** — see
    // [`crate::model::EmbeddedFont`]'s own doc comment.
    if !embedded_fonts.is_empty() {
        writer.write_event(Event::Start(BytesStart::new("p:embeddedFontLst")))?;
        for font in embedded_fonts {
            writer.write_event(Event::Start(BytesStart::new("p:embeddedFont")))?;

            let mut font_element = BytesStart::new("p:font");
            font_element.push_attribute(("typeface", font.typeface.as_str()));
            writer.write_event(Event::Empty(font_element))?;

            let variants: [(&str, &Option<String>); 4] = [
                ("p:regular", &font.regular),
                ("p:bold", &font.bold),
                ("p:italic", &font.italic),
                ("p:boldItalic", &font.bold_italic),
            ];
            for (element_name, relationship_id) in variants {
                if let Some(relationship_id) = relationship_id {
                    let mut variant = BytesStart::new(element_name);
                    variant.push_attribute(("r:id", relationship_id.as_str()));
                    writer.write_event(Event::Empty(variant))?;
                }
            }

            writer.write_event(Event::End(BytesEnd::new("p:embeddedFont")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("p:embeddedFontLst")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("p:presentation")))?;
    Ok(writer.into_inner())
}

/// Serializes one `ppt/slides/slideN.xml` (`CT_Slide`) from a [`Slide`].
fn to_slide_xml(
    slide: &Slide,
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:sld");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    if slide.hidden {
        root.push_attribute(("show", "0"));
    }
    if slide.hide_master_graphics {
        root.push_attribute(("showMasterSp", "0"));
    }
    writer.write_event(Event::Start(root))?;

    let mut c_sld = BytesStart::new("p:cSld");
    if let Some(name) = &slide.name {
        c_sld.push_attribute(("name", name.as_str()));
    }
    writer.write_event(Event::Start(c_sld))?;
    if let Some(background) = &slide.background {
        write_slide_background(&mut writer, background)?;
    }
    write_shape_tree(&mut writer, &slide.shapes, state, relationships)?;
    writer.write_event(Event::End(BytesEnd::new("p:cSld")))?;

    write_color_map_override(&mut writer)?;

    writer.write_event(Event::End(BytesEnd::new("p:sld")))?;
    Ok(writer.into_inner())
}

/// Writes `<p:bg><p:bgPr>`'s own `EG_FillProperties` choice `</p:bgPr></p:bg>`
/// (`CT_BackgroundProperties`, point 4) — `<p:cSld>`'s own first, optional child (`bg?`, before
/// `spTree`). `shadeToTitle` and `<a:effectLst>`/`<a:effectDag>` are not modeled, same "direct fill
/// only, no theme background reference" scope as [`crate::model::Slide::background`]'s own doc
/// comment.
fn write_slide_background<W: Write>(
    writer: &mut Writer<W>,
    background: &drawing::Fill,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:bg")))?;
    writer.write_event(Event::Start(BytesStart::new("p:bgPr")))?;
    drawing::write_fill(writer, background)?;
    writer.write_event(Event::End(BytesEnd::new("p:bgPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:bg")))?;
    Ok(())
}

/// Writes `<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>` — every slide and slide layout this
/// crate writes simply inherits its color map from the (single, fixed) slide master, rather than
/// overriding it.
fn write_color_map_override<W: Write>(writer: &mut Writer<W>) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:clrMapOvr")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:masterClrMapping")))?;
    writer.write_event(Event::End(BytesEnd::new("p:clrMapOvr")))?;
    Ok(())
}

/// Writes `<p:spTree>`: the mandatory group-shape header (`p:nvGrpSpPr`/`p:grpSpPr`, both
/// fixed/empty — this crate has no notion of nested shape groups yet, see [`Shape`]'s doc comment)
/// followed by one child per shape, dispatched by kind.
fn write_shape_tree<W: Write>(
    writer: &mut Writer<W>,
    shapes: &[Shape],
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:spTree")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvGrpSpPr")))?;
    let mut group_cnv_pr = BytesStart::new("p:cNvPr");
    group_cnv_pr.push_attribute(("id", "1"));
    group_cnv_pr.push_attribute(("name", ""));
    writer.write_event(Event::Empty(group_cnv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvGrpSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGrpSpPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:grpSpPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    write_zero_point(writer, "a:off", "x", "y")?;
    write_zero_point(writer, "a:ext", "cx", "cy")?;
    write_zero_point(writer, "a:chOff", "x", "y")?;
    write_zero_point(writer, "a:chExt", "cx", "cy")?;
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    writer.write_event(Event::End(BytesEnd::new("p:grpSpPr")))?;

    write_shapes(writer, shapes, state, relationships)?;

    writer.write_event(Event::End(BytesEnd::new("p:spTree")))?;
    Ok(())
}

/// Dispatches and writes each shape in document order — the part shared by `<p:spTree>` (the
/// slide's own top-level shape list, via [`write_shape_tree`]) and `<p:grpSp>` (a group's own
/// nested shape list, via [`write_group_shape`]), since both are just "one child element per
/// [`Shape`]" once their own fixed/real header is out of the way.
fn write_shapes<W: Write>(
    writer: &mut Writer<W>,
    shapes: &[Shape],
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    for shape in shapes {
        match shape {
            Shape::AutoShape(auto_shape) => write_auto_shape(writer, auto_shape, relationships)?,
            Shape::Picture(picture) => write_picture(writer, picture, state, relationships)?,
            Shape::Chart(slide_chart) => {
                write_slide_chart(writer, slide_chart, state, relationships)?
            }
            Shape::Group(group) => write_group_shape(writer, group, state, relationships)?,
            Shape::Connector(connector) => write_connector(writer, connector)?,
            Shape::Table(table) => write_table(writer, table)?,
            Shape::Media(media) => write_media_shape(writer, media, state, relationships)?,
            Shape::SmartArt(smartart) => write_diagram_shape(writer, smartart)?,
        }
    }
    Ok(())
}

/// Writes a single zero-valued two-attribute empty element, e.g. `<a:off x="0" y="0"/>` or `<a:ext
/// cx="0" cy="0"/>`.
fn write_zero_point<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    first_attribute: &str,
    second_attribute: &str,
) -> Result<()> {
    let mut start = BytesStart::new(element_name);
    start.push_attribute((first_attribute, "0"));
    start.push_attribute((second_attribute, "0"));
    writer.write_event(Event::Empty(start))?;
    Ok(())
}

/// Writes a single `<p:sp>` (`CT_Shape`) from an [`AutoShape`]. Registers a hyperlink relationship
/// (external, or internal for an in-package slide jump) when `shape.hyperlink` is set; see
/// [`write_hyperlink_click`].
fn write_auto_shape<W: Write>(
    writer: &mut Writer<W>,
    shape: &AutoShape,
    relationships: &mut PartRelationships,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:sp")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvSpPr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", shape.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", shape.name.as_str()));
    match &shape.hyperlink {
        None => {
            writer.write_event(Event::Empty(cnv_pr))?;
        }
        Some(target) => {
            writer.write_event(Event::Start(cnv_pr))?;
            write_hyperlink_click(writer, target, relationships)?;
            writer.write_event(Event::End(BytesEnd::new("p:cNvPr")))?;
        }
    }

    let mut cnv_sp_pr = BytesStart::new("p:cNvSpPr");
    if shape.is_text_box {
        cnv_sp_pr.push_attribute(("txBox", "1"));
    }
    writer.write_event(Event::Empty(cnv_sp_pr))?;
    write_placeholder(writer, shape.placeholder.as_ref())?;
    writer.write_event(Event::End(BytesEnd::new("p:nvSpPr")))?;

    write_shape_properties(writer, &shape.properties)?;

    if let Some(text_body) = &shape.text_body {
        writer.write_event(Event::Start(BytesStart::new("p:txBody")))?;
        drawing::write_text_body(writer, text_body)?;
        writer.write_event(Event::End(BytesEnd::new("p:txBody")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

/// Writes `<a:hlinkClick./>` for any [`SlideHyperlinkTarget`]. [`SlideHyperlinkTarget::External`]
/// registers an external relationship and writes `r:id` alone.
/// [`SlideHyperlinkTarget::Slide`] registers an *internal* relationship targeting
/// `"./slides/slide{index + 1}.xml"` (resolvable purely from the index — see
/// [`SlideHyperlinkTarget::Slide`]'s own doc comment) and pairs `r:id` with
/// `action="ppaction://hlinksldjump"`, matching a real fixture. The four relative-jump variants
/// need no relationship at all: no `r:id`, just `action="ppaction://hlinkshowjump? jump=."`,
/// modeled directly from ECMA-376's documented action values (no fixture in this project's corpus
/// exercises them).
fn write_hyperlink_click<W: Write>(
    writer: &mut Writer<W>,
    target: &SlideHyperlinkTarget,
    relationships: &mut PartRelationships,
) -> Result<()> {
    let mut hlink = BytesStart::new("a:hlinkClick");
    match target {
        SlideHyperlinkTarget::External(url) => {
            let relationship_id =
                relationships.add_external(HYPERLINK_RELATIONSHIP_TYPE, url.as_str());
            hlink.push_attribute(("r:id", relationship_id.as_str()));
        }
        SlideHyperlinkTarget::Slide(index) => {
            let relationship_id = relationships.add(
                SLIDE_RELATIONSHIP_TYPE,
                format!("../slides/slide{}.xml", index + 1),
            );
            hlink.push_attribute(("r:id", relationship_id.as_str()));
            hlink.push_attribute(("action", "ppaction://hlinksldjump"));
        }
        SlideHyperlinkTarget::NextSlide => {
            hlink.push_attribute(("action", "ppaction://hlinkshowjump?jump=nextslide"));
        }
        SlideHyperlinkTarget::PreviousSlide => {
            hlink.push_attribute(("action", "ppaction://hlinkshowjump?jump=previousslide"));
        }
        SlideHyperlinkTarget::FirstSlide => {
            hlink.push_attribute(("action", "ppaction://hlinkshowjump?jump=firstslide"));
        }
        SlideHyperlinkTarget::LastSlide => {
            hlink.push_attribute(("action", "ppaction://hlinkshowjump?jump=lastslide"));
        }
    }
    writer.write_event(Event::Empty(hlink))?;
    Ok(())
}

/// Writes `<p:nvPr>`, empty unless a placeholder role is set, in which case it wraps a `<p:ph
/// type=".." idx="..">` (`CT_Placeholder`).
fn write_placeholder<W: Write>(
    writer: &mut Writer<W>,
    placeholder: Option<&Placeholder>,
) -> Result<()> {
    match placeholder {
        None => {
            writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
        }
        Some(placeholder) => {
            writer.write_event(Event::Start(BytesStart::new("p:nvPr")))?;
            let mut placeholder_start = BytesStart::new("p:ph");
            placeholder_start.push_attribute(("type", placeholder.kind.xml_token()));
            if let Some(index) = placeholder.index {
                placeholder_start.push_attribute(("idx", index.to_string().as_str()));
            }
            writer.write_event(Event::Empty(placeholder_start))?;
            writer.write_event(Event::End(BytesEnd::new("p:nvPr")))?;
        }
    }
    Ok(())
}

/// Writes `<p:spPr>` — always with an explicit `<a:xfrm>` and `<a:prstGeom>`, even when
/// [`ShapeProperties::transform`]/`::geometry` are unset (a fixed zero-sized rect is written
/// instead).
///
/// This mirrors a fix `excel-ooxml`'s picture writing needed twice this project (see
/// `CHANGELOG.md`, "Corriger bug xdr:spPr sans géométrie" / "Ajouter a:xfrm à xdr:pic"): delegating
/// entirely to `drawing::write_shape_properties` (which only emits `xfrm`/`prstGeom` when the
/// caller actually set them) produced a shape real Office silently refused to render at all.
/// Applied here from the start, rather than rediscovered. consolidation: the geometry-fallback +
/// fill/line/effects part (everything but the `xfrm`, which differs by host format) now delegates
/// to `drawing::write_geometry_fill_line_effects` instead of a hand-rolled copy — this crate's own
/// three former copies of that logic (here, [`write_picture`], [`write_connector_properties`]) were
/// the only ones that hadn't drifted (they already wrote `effects`; `word-ooxml`'s and
/// `excel-ooxml`'s otherwise-identical copies hadn't).
fn write_shape_properties<W: Write>(
    writer: &mut Writer<W>,
    properties: &ShapeProperties,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;

    match &properties.transform {
        Some(transform) => drawing::write_transform(writer, transform)?,
        None => {
            writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
            write_zero_point(writer, "a:off", "x", "y")?;
            write_zero_point(writer, "a:ext", "cx", "cy")?;
            writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
        }
    }

    drawing::write_geometry_fill_line_effects(writer, properties, "rect")?;

    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    Ok(())
}

/// Writes one `<p:pic>` (`CT_Picture`) from a [`crate::model::Picture`]. Registers the image as a
/// package-wide-unique `ppt/media/imageN.*` part (`state`) and a slide-scoped relationship
/// (`relationships`) along the way — structure grounded in the same real-Excel/real-Word/real-
/// PowerPoint cross-check this project has now applied three times (`nvPicPr`/`blipFill`/`spPr`, in
/// that order; `spPr` always carries a real `<a:xfrm>`, since — unlike Excel's cell-anchored
/// pictures — a PresentationML picture's position/size lives nowhere else).
fn write_picture<W: Write>(
    writer: &mut Writer<W>,
    picture: &crate::model::Picture,
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    let entry_name = state.media.register(picture);
    let relationship_id = relationships.add(IMAGE_RELATIONSHIP_TYPE, format!("../{entry_name}"));

    writer.write_event(Event::Start(BytesStart::new("p:pic")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvPicPr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", picture.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", picture.name.as_str()));
    cnv_pr.push_attribute(("descr", picture.description.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Start(BytesStart::new("p:cNvPicPr")))?;
    let mut pic_locks = BytesStart::new("a:picLocks");
    pic_locks.push_attribute(("noChangeAspect", "1"));
    writer.write_event(Event::Empty(pic_locks))?;
    writer.write_event(Event::End(BytesEnd::new("p:cNvPicPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvPicPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:blipFill")))?;
    let mut blip = BytesStart::new("a:blip");
    blip.push_attribute(("r:embed", relationship_id.as_str()));
    // An additional `r:link`, alongside `r:embed`, when this picture also has an external source
    // (PowerPoint's own "embedded preview cache + externally-linked original" dual pattern; see
    // [`crate::model::Picture::external_link`]'s own doc comment). Shares `IMAGE_RELATIONSHIP_TYPE`
    // with `r:embed` — real packages use the same relationship type for both, only `TargetMode`
    // differs.
    if let Some(external_link) = &picture.external_link {
        let link_relationship_id =
            relationships.add_external(IMAGE_RELATIONSHIP_TYPE, external_link.as_str());
        blip.push_attribute(("r:link", link_relationship_id.as_str()));
    }
    writer.write_event(Event::Empty(blip))?;
    writer.write_event(Event::Start(BytesStart::new("a:stretch")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:fillRect")))?;
    writer.write_event(Event::End(BytesEnd::new("a:stretch")))?;
    writer.write_event(Event::End(BytesEnd::new("p:blipFill")))?;

    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", picture.offset_emu.0.to_string().as_str()));
    off.push_attribute(("y", picture.offset_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", picture.extent_emu.0.to_string().as_str()));
    ext.push_attribute(("cy", picture.extent_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;

    let empty_properties = drawing::ShapeProperties::default();
    let shape_properties = picture
        .shape_properties
        .as_ref()
        .unwrap_or(&empty_properties);
    drawing::write_geometry_fill_line_effects(writer, shape_properties, "rect")?;
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;

    writer.write_event(Event::End(BytesEnd::new("p:pic")))?;
    Ok(())
}

/// Writes one `<p:pic>` carrying an `<a:videoFile>`/`<a:audioFile>` reference, from a
/// [`SlideMedia`]. Registers the media bytes as a package-wide-unique `ppt/media/mediaN.*` part and
/// a slide-scoped `video`/`audio`-typed relationship (`r:link`, resolved by
/// [`crate::model::MediaFormat::is_video`]); the poster frame, if set, is registered and referenced
/// the same way an ordinary [`Picture`]'s own image is. Does **not** write the
/// Microsoft-proprietary `p14:media` extension real PowerPoint 2010+ also emits — see
/// [`SlideMedia`]'s own doc comment.
fn write_media_shape<W: Write>(
    writer: &mut Writer<W>,
    media: &SlideMedia,
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    let media_entry_name = state.media.register_bytes(
        "media",
        &media.data,
        media.format.extension(),
        media.format.content_type(),
    );
    let media_relationship_type = if media.format.is_video() {
        VIDEO_RELATIONSHIP_TYPE
    } else {
        AUDIO_RELATIONSHIP_TYPE
    };
    let media_relationship_id =
        relationships.add(media_relationship_type, format!("../{media_entry_name}"));

    writer.write_event(Event::Start(BytesStart::new("p:pic")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvPicPr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", media.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", media.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Start(BytesStart::new("p:cNvPicPr")))?;
    let mut pic_locks = BytesStart::new("a:picLocks");
    pic_locks.push_attribute(("noChangeAspect", "1"));
    writer.write_event(Event::Empty(pic_locks))?;
    writer.write_event(Event::End(BytesEnd::new("p:cNvPicPr")))?;
    writer.write_event(Event::Start(BytesStart::new("p:nvPr")))?;
    let media_element_name = if media.format.is_video() {
        "a:videoFile"
    } else {
        "a:audioFile"
    };
    let mut media_start = BytesStart::new(media_element_name);
    media_start.push_attribute(("r:link", media_relationship_id.as_str()));
    writer.write_event(Event::Empty(media_start))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvPicPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:blipFill")))?;
    if let Some((poster_data, poster_format)) = &media.poster_image {
        let poster_entry_name = state.media.register_bytes(
            "image",
            poster_data,
            poster_format.extension(),
            poster_format.content_type(),
        );
        let poster_relationship_id =
            relationships.add(IMAGE_RELATIONSHIP_TYPE, format!("../{poster_entry_name}"));
        let mut blip = BytesStart::new("a:blip");
        blip.push_attribute(("r:embed", poster_relationship_id.as_str()));
        writer.write_event(Event::Empty(blip))?;
    }
    writer.write_event(Event::Start(BytesStart::new("a:stretch")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:fillRect")))?;
    writer.write_event(Event::End(BytesEnd::new("a:stretch")))?;
    writer.write_event(Event::End(BytesEnd::new("p:blipFill")))?;

    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    write_point(writer, "a:off", "x", "y", media.offset_emu)?;
    write_point(writer, "a:ext", "cx", "cy", media.extent_emu)?;
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    drawing::write_geometry_fill_line_effects(
        writer,
        &drawing::ShapeProperties::default(),
        "rect",
    )?;
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;

    writer.write_event(Event::End(BytesEnd::new("p:pic")))?;
    Ok(())
}

/// Writes one `<p:graphicFrame>` (`CT_GraphicalObjectFrame`) wrapping a `<c:chart r:id="..">`, from
/// a [`crate::model::SlideChart`]. Registers the chart as a package-wide-unique
/// `ppt/charts/chartN.xml` part (`state`) and a slide-scoped relationship (`relationships`). Note
/// the transform element here is `<p:xfrm>`, not `<a:xfrm>`/`<xdr:xfrm>` — confirmed against a real
/// chart-bearing `.pptx` fixture — and, unlike Excel's `SheetChart` (whose `xdr:xfrm` is a fixed,
/// meaningless placeholder, real positioning coming from the enclosing `xdr:twoCellAnchor`), this
/// one carries the chart's *real* position/ size, the same posture as [`write_picture`]'s own
/// `<a:xfrm>`.
fn write_slide_chart<W: Write>(
    writer: &mut Writer<W>,
    slide_chart: &crate::model::SlideChart,
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    let entry_name = state.charts.register(&slide_chart.chart_space)?;
    let relationship_id = relationships.add(CHART_RELATIONSHIP_TYPE, format!("../{entry_name}"));

    writer.write_event(Event::Start(BytesStart::new("p:graphicFrame")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvGraphicFramePr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", slide_chart.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", slide_chart.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvGraphicFramePr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGraphicFramePr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:xfrm")))?;
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", slide_chart.offset_emu.0.to_string().as_str()));
    off.push_attribute(("y", slide_chart.offset_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", slide_chart.extent_emu.0.to_string().as_str()));
    ext.push_attribute(("cy", slide_chart.extent_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    writer.write_event(Event::End(BytesEnd::new("p:xfrm")))?;

    writer.write_event(Event::Start(BytesStart::new("a:graphic")))?;
    let mut graphic_data = BytesStart::new("a:graphicData");
    graphic_data.push_attribute(("uri", CHART_NAMESPACE));
    writer.write_event(Event::Start(graphic_data))?;
    let mut c_chart = BytesStart::new("c:chart");
    c_chart.push_attribute(("xmlns:c", CHART_NAMESPACE));
    c_chart.push_attribute(("r:id", relationship_id.as_str()));
    writer.write_event(Event::Empty(c_chart))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphicData")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphic")))?;

    writer.write_event(Event::End(BytesEnd::new("p:graphicFrame")))?;
    Ok(())
}

/// Writes a single SmartArt diagram frame `<p:graphicFrame>` in *degraded* form: the frame's
/// own identity, geometry, and the diagram namespace, but none of the diagram's data parts
/// (this crate models none of them — see the [`Shape::SmartArt`](crate::model::Shape::SmartArt)
/// doc). The `<dgm:relIds>` is emitted with no attributes so the frame stays schema-valid while
/// declaring no diagram data; PowerPoint opens it as an empty diagram frame asking for an
/// update, and the shape's position/extent survive intact — the contract this variant promises.
fn write_diagram_shape<W: Write>(
    writer: &mut Writer<W>,
    smartart: &crate::model::SmartArt,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:graphicFrame")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvGraphicFramePr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", smartart.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", smartart.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvGraphicFramePr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGraphicFramePr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:xfrm")))?;
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", smartart.offset_emu.0.to_string().as_str()));
    off.push_attribute(("y", smartart.offset_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", smartart.extent_emu.0.to_string().as_str()));
    ext.push_attribute(("cy", smartart.extent_emu.1.to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    writer.write_event(Event::End(BytesEnd::new("p:xfrm")))?;

    writer.write_event(Event::Start(BytesStart::new("a:graphic")))?;
    let mut graphic_data = BytesStart::new("a:graphicData");
    graphic_data.push_attribute(("uri", DIAGRAM_NAMESPACE));
    writer.write_event(Event::Start(graphic_data))?;
    let mut dgm_rel_ids = BytesStart::new("dgm:relIds");
    dgm_rel_ids.push_attribute(("xmlns:dgm", DIAGRAM_NAMESPACE));
    writer.write_event(Event::Empty(dgm_rel_ids))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphicData")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphic")))?;

    writer.write_event(Event::End(BytesEnd::new("p:graphicFrame")))?;
    Ok(())
}

/// Writes one `<p:grpSp>` (`CT_GroupShape`) from a [`ShapeGroup`]: its own non-visual header, a
/// *real* `<p:grpSpPr>/<a:xfrm>` (unlike [`write_shape_tree`]'s always-zero one for the slide's own
/// top-level `<p:spTree>`), then its children via [`write_shapes`] — recursing naturally for a
/// nested group. Structure confirmed against a real fixture with nested groups.
fn write_group_shape<W: Write>(
    writer: &mut Writer<W>,
    group: &ShapeGroup,
    state: &mut PackageState,
    relationships: &mut PartRelationships,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:grpSp")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvGrpSpPr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", group.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", group.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvGrpSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGrpSpPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:grpSpPr")))?;
    let mut xfrm = BytesStart::new("a:xfrm");
    if group.rotation_60000ths != 0 {
        xfrm.push_attribute(("rot", group.rotation_60000ths.to_string().as_str()));
    }
    if group.flip_horizontal {
        xfrm.push_attribute(("flipH", "1"));
    }
    if group.flip_vertical {
        xfrm.push_attribute(("flipV", "1"));
    }
    writer.write_event(Event::Start(xfrm))?;
    write_point(writer, "a:off", "x", "y", group.offset_emu)?;
    write_point(writer, "a:ext", "cx", "cy", group.extent_emu)?;
    write_point(writer, "a:chOff", "x", "y", group.child_offset_emu)?;
    write_point(writer, "a:chExt", "cx", "cy", group.child_extent_emu)?;
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    writer.write_event(Event::End(BytesEnd::new("p:grpSpPr")))?;

    write_shapes(writer, &group.shapes, state, relationships)?;

    writer.write_event(Event::End(BytesEnd::new("p:grpSp")))?;
    Ok(())
}

/// Writes a single two-attribute empty element with real (not necessarily zero) values, e.g.
/// `<a:off x="123" y="456"/>` — generalizes [`write_zero_point`] for callers (like
/// [`write_group_shape`]) that need a genuine, non-zero transform.
fn write_point<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    first_attribute: &str,
    second_attribute: &str,
    (first, second): (i64, i64),
) -> Result<()> {
    let mut start = BytesStart::new(element_name);
    start.push_attribute((first_attribute, first.to_string().as_str()));
    start.push_attribute((second_attribute, second.to_string().as_str()));
    writer.write_event(Event::Empty(start))?;
    Ok(())
}

/// Writes one `<p:cxnSp>` (`CT_Connector`) from a [`Connector`]. **Not grounded on a real fixture**
/// — see `model.rs`'s top-level doc comment and [`Connector`]'s own doc comment.
fn write_connector<W: Write>(writer: &mut Writer<W>, connector: &Connector) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:cxnSp")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvCxnSpPr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", connector.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", connector.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;

    match (&connector.start_connection, &connector.end_connection) {
        (None, None) => {
            writer.write_event(Event::Empty(BytesStart::new("p:cNvCxnSpPr")))?;
        }
        _ => {
            writer.write_event(Event::Start(BytesStart::new("p:cNvCxnSpPr")))?;
            write_shape_connection(writer, "a:stCxn", connector.start_connection.as_ref())?;
            write_shape_connection(writer, "a:endCxn", connector.end_connection.as_ref())?;
            writer.write_event(Event::End(BytesEnd::new("p:cNvCxnSpPr")))?;
        }
    }
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvCxnSpPr")))?;

    write_connector_properties(writer, &connector.properties)?;

    writer.write_event(Event::End(BytesEnd::new("p:cxnSp")))?;
    Ok(())
}

/// Writes `<a:stCxn>`/`<a:endCxn>` (`CT_Connection`) if `connection` is set; writes nothing
/// otherwise (the caller only enters this branch when at least one of the pair is set, but each is
/// independently optional).
fn write_shape_connection<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    connection: Option<&crate::model::ShapeConnection>,
) -> Result<()> {
    if let Some(connection) = connection {
        let mut start = BytesStart::new(element_name);
        start.push_attribute(("id", connection.shape_id.to_string().as_str()));
        start.push_attribute(("idx", connection.index.to_string().as_str()));
        writer.write_event(Event::Empty(start))?;
    }
    Ok(())
}

/// Writes `<p:spPr>` for a connector — same as [`write_shape_properties`] but defaults to
/// `<a:prstGeom prst="line">` (a straight connector) rather than `"rect"` when
/// `properties.geometry` is unset, the connector-appropriate equivalent of the same "always write
/// geometry explicitly" discipline.
fn write_connector_properties<W: Write>(
    writer: &mut Writer<W>,
    properties: &ShapeProperties,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;

    match &properties.transform {
        Some(transform) => drawing::write_transform(writer, transform)?,
        None => {
            writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
            write_zero_point(writer, "a:off", "x", "y")?;
            write_zero_point(writer, "a:ext", "cx", "cy")?;
            writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
        }
    }

    drawing::write_geometry_fill_line_effects(writer, properties, "line")?;

    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    Ok(())
}

/// Writes one `<p:graphicFrame>` wrapping an `<a:tbl>` (`CT_Table`) from a [`SlideTable`].
/// Structure confirmed against real fixtures — note the frame's own transform element is
/// `<p:xfrm>`, same as [`write_slide_chart`]'s.
///
/// Every row is padded (with empty [`TableCell`]s) or truncated to exactly
/// `table.column_widths_emu.len()` cells — real PowerPoint requires one `<a:tc>` per `<a:gridCol>`
/// in every row (unlike WordprocessingML tables, where a spanned cell is simply omitted, see
/// [`TableCell`]'s own doc comment) — so a caller-supplied row that's too short/long never produces
/// a schema-invalid `<a:tbl>`.
fn write_table<W: Write>(writer: &mut Writer<W>, table: &SlideTable) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new("p:graphicFrame")))?;

    writer.write_event(Event::Start(BytesStart::new("p:nvGraphicFramePr")))?;
    let mut cnv_pr = BytesStart::new("p:cNvPr");
    cnv_pr.push_attribute(("id", table.id.to_string().as_str()));
    cnv_pr.push_attribute(("name", table.name.as_str()));
    writer.write_event(Event::Empty(cnv_pr))?;
    writer.write_event(Event::Start(BytesStart::new("p:cNvGraphicFramePr")))?;
    let mut locks = BytesStart::new("a:graphicFrameLocks");
    locks.push_attribute(("noGrp", "1"));
    writer.write_event(Event::Empty(locks))?;
    writer.write_event(Event::End(BytesEnd::new("p:cNvGraphicFramePr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGraphicFramePr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:xfrm")))?;
    write_point(writer, "a:off", "x", "y", table.offset_emu)?;
    write_point(writer, "a:ext", "cx", "cy", table.extent_emu)?;
    writer.write_event(Event::End(BytesEnd::new("p:xfrm")))?;

    writer.write_event(Event::Start(BytesStart::new("a:graphic")))?;
    let mut graphic_data = BytesStart::new("a:graphicData");
    graphic_data.push_attribute(("uri", TABLE_NAMESPACE));
    writer.write_event(Event::Start(graphic_data))?;

    writer.write_event(Event::Start(BytesStart::new("a:tbl")))?;
    let mut tbl_pr = BytesStart::new("a:tblPr");
    if table.style_first_row {
        tbl_pr.push_attribute(("firstRow", "1"));
    }
    if table.style_first_column {
        tbl_pr.push_attribute(("firstCol", "1"));
    }
    if table.style_last_row {
        tbl_pr.push_attribute(("lastRow", "1"));
    }
    if table.style_last_column {
        tbl_pr.push_attribute(("lastCol", "1"));
    }
    if table.style_band_rows {
        tbl_pr.push_attribute(("bandRow", "1"));
    }
    if table.style_band_columns {
        tbl_pr.push_attribute(("bandCol", "1"));
    }
    writer.write_event(Event::Start(tbl_pr))?;
    let table_style_id = table.style_id.as_deref().unwrap_or(TABLE_STYLE_GUID);
    validate_table_style_id(table_style_id)?;
    writer.write_event(Event::Start(BytesStart::new("a:tableStyleId")))?;
    writer.write_event(Event::Text(xml_core::BytesText::new(table_style_id)))?;
    writer.write_event(Event::End(BytesEnd::new("a:tableStyleId")))?;
    writer.write_event(Event::End(BytesEnd::new("a:tblPr")))?;

    let column_count = table.column_widths_emu.len();
    writer.write_event(Event::Start(BytesStart::new("a:tblGrid")))?;
    for width_emu in &table.column_widths_emu {
        let mut grid_col = BytesStart::new("a:gridCol");
        grid_col.push_attribute(("w", width_emu.to_string().as_str()));
        writer.write_event(Event::Empty(grid_col))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:tblGrid")))?;

    for row in &table.rows {
        write_table_row(writer, row, column_count)?;
    }

    writer.write_event(Event::End(BytesEnd::new("a:tbl")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphicData")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphic")))?;

    writer.write_event(Event::End(BytesEnd::new("p:graphicFrame")))?;
    Ok(())
}

/// Writes one `<a:tr h="..">` (`CT_TableRow`), padding/truncating its cells to exactly
/// `column_count` — see [`write_table`]'s own doc comment.
fn write_table_row<W: Write>(
    writer: &mut Writer<W>,
    row: &TableRow,
    column_count: usize,
) -> Result<()> {
    let mut tr = BytesStart::new("a:tr");
    tr.push_attribute(("h", row.height_emu.to_string().as_str()));
    writer.write_event(Event::Start(tr))?;

    for index in 0..column_count {
        match row.cells.get(index) {
            Some(cell) => write_table_cell(writer, cell)?,
            None => write_table_cell(writer, &TableCell::new())?,
        }
    }

    writer.write_event(Event::End(BytesEnd::new("a:tr")))?;
    Ok(())
}

/// Writes one `<a:tc>` (`CT_TableCell`) — always with a real `<a:txBody>` (mandatory per the
/// schema, unlike an autoshape's own optional `p:txBody>`) and an `<a:tcPr>` reflecting
/// `cell.properties` (`<a:tcPr/>` self-closes, matching every real fixture checked for a
/// plainly-styled cell, when `properties` is `None`).
fn write_table_cell<W: Write>(writer: &mut Writer<W>, cell: &TableCell) -> Result<()> {
    let mut tc = BytesStart::new("a:tc");
    if let Some(span) = cell.horizontal_span {
        tc.push_attribute(("gridSpan", span.to_string().as_str()));
    }
    if let Some(span) = cell.vertical_span {
        tc.push_attribute(("rowSpan", span.to_string().as_str()));
    }
    if cell.horizontal_merge {
        tc.push_attribute(("hMerge", "1"));
    }
    if cell.vertical_merge {
        tc.push_attribute(("vMerge", "1"));
    }
    writer.write_event(Event::Start(tc))?;

    writer.write_event(Event::Start(BytesStart::new("a:txBody")))?;
    match &cell.text_body {
        Some(text_body) => drawing::write_text_body(writer, text_body)?,
        None => {
            // `<a:txBody>` is mandatory (`minOccurs="1"` on `CT_TableCell`) even for a genuinely
            // empty cell — write a minimal but schema-complete body/paragraph rather than an
            // invalid empty wrapper.
            drawing::write_text_body(
                writer,
                &drawing::TextBody::new().with_paragraph(drawing::TextParagraph::new()),
            )?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new("a:txBody")))?;
    write_table_cell_properties(writer, cell.properties.as_ref())?;

    writer.write_event(Event::End(BytesEnd::new("a:tc")))?;
    Ok(())
}

/// Writes `<a:tcPr>` (`CT_TableCellProperties`, point 8) — self-closing when `properties` is
/// `None`, matching the fixed behavior every fixture this crate checked uses for a plainly-styled
/// cell. Attribute order (`marL`/`marT`/`marR`/`marB`/`anchor`) and child order
/// (`lnL`/`lnR`/`lnT`/`lnB` then the fill choice) follow `CT_TableCellProperties`'s own schema
/// sequence, confirmed against a real fixture.
fn write_table_cell_properties<W: Write>(
    writer: &mut Writer<W>,
    properties: Option<&TableCellProperties>,
) -> Result<()> {
    let Some(properties) = properties else {
        writer.write_event(Event::Empty(BytesStart::new("a:tcPr")))?;
        return Ok(());
    };

    let mut start = BytesStart::new("a:tcPr");
    if let Some(margin) = properties.margin_left_emu {
        start.push_attribute(("marL", margin.to_string().as_str()));
    }
    if let Some(margin) = properties.margin_top_emu {
        start.push_attribute(("marT", margin.to_string().as_str()));
    }
    if let Some(margin) = properties.margin_right_emu {
        start.push_attribute(("marR", margin.to_string().as_str()));
    }
    if let Some(margin) = properties.margin_bottom_emu {
        start.push_attribute(("marB", margin.to_string().as_str()));
    }
    if let Some(anchor) = &properties.anchor {
        start.push_attribute(("anchor", table_cell_anchor_token(anchor)));
    }

    let has_children = properties.border_left.is_some()
        || properties.border_right.is_some()
        || properties.border_top.is_some()
        || properties.border_bottom.is_some()
        || properties.fill.is_some();
    if !has_children {
        writer.write_event(Event::Empty(start))?;
        return Ok(());
    }

    writer.write_event(Event::Start(start))?;
    if let Some(border) = &properties.border_left {
        drawing::write_line_named(writer, "a:lnL", border)?;
    }
    if let Some(border) = &properties.border_right {
        drawing::write_line_named(writer, "a:lnR", border)?;
    }
    if let Some(border) = &properties.border_top {
        drawing::write_line_named(writer, "a:lnT", border)?;
    }
    if let Some(border) = &properties.border_bottom {
        drawing::write_line_named(writer, "a:lnB", border)?;
    }
    if let Some(fill) = &properties.fill {
        drawing::write_fill(writer, fill)?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:tcPr")))?;
    Ok(())
}

/// `ST_TextAnchoringType`'s own XML token, duplicated locally since
/// [`drawing::TextAnchor::xml_token`] is crate-private to `drawing` itself (only
/// [`drawing::write_text_body_properties`] needs it there — this is the first cross-crate need for
/// the same mapping, on `<a:tcPr anchor= "..">` rather than `<a:bodyPr anchor="..">`, same
/// `pub(crate)`-per- module posture already used for e.g. `SLIDE_RELATIONSHIP_TYPE`).
fn table_cell_anchor_token(anchor: &TextAnchor) -> &'static str {
    match anchor {
        TextAnchor::Top => "t",
        TextAnchor::Center => "ctr",
        TextAnchor::Bottom => "b",
        TextAnchor::Justified => "just",
        TextAnchor::Distributed => "dist",
    }
}

/// Serializes the single fixed `ppt/slideMasters/slideMaster1.xml` (`CT_SlideMaster`) this crate
/// ever writes: an empty shape tree (no placeholder shapes — this crate's slides carry their own
/// placeholder info directly, see `model.rs`), the standard default color map, and a
/// `<p:sldLayoutIdLst>` with the one fixed slide layout this crate writes.
fn to_slide_master_xml() -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:sldMaster");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    writer.write_event(Event::Start(root))?;

    writer.write_event(Event::Start(BytesStart::new("p:cSld")))?;
    write_shape_tree(
        &mut writer,
        &[],
        &mut PackageState::new(),
        &mut PartRelationships::new(),
    )?;
    writer.write_event(Event::End(BytesEnd::new("p:cSld")))?;

    write_default_color_map(&mut writer)?;

    writer.write_event(Event::Start(BytesStart::new("p:sldLayoutIdLst")))?;
    let mut layout_id = BytesStart::new("p:sldLayoutId");
    layout_id.push_attribute(("id", (SLIDE_MASTER_ID + 1).to_string().as_str()));
    layout_id.push_attribute(("r:id", "rId1"));
    writer.write_event(Event::Empty(layout_id))?;
    writer.write_event(Event::End(BytesEnd::new("p:sldLayoutIdLst")))?;

    writer.write_event(Event::End(BytesEnd::new("p:sldMaster")))?;
    Ok(writer.into_inner())
}

/// Writes `<p:clrMap>`'s fixed default mapping (every slot maps to itself — `bg1="lt1"`,
/// `tx1="dk1"`. — the same one real PowerPoint uses for a brand new default deck). Also the exact
/// mapping a real notes master uses, so [`to_notes_master_xml`] reuses this same function.
fn write_default_color_map<W: Write>(writer: &mut Writer<W>) -> Result<()> {
    let mut clr_map = BytesStart::new("p:clrMap");
    clr_map.push_attribute(("bg1", "lt1"));
    clr_map.push_attribute(("tx1", "dk1"));
    clr_map.push_attribute(("bg2", "lt2"));
    clr_map.push_attribute(("tx2", "dk2"));
    clr_map.push_attribute(("accent1", "accent1"));
    clr_map.push_attribute(("accent2", "accent2"));
    clr_map.push_attribute(("accent3", "accent3"));
    clr_map.push_attribute(("accent4", "accent4"));
    clr_map.push_attribute(("accent5", "accent5"));
    clr_map.push_attribute(("accent6", "accent6"));
    clr_map.push_attribute(("hlink", "hlink"));
    clr_map.push_attribute(("folHlink", "folHlink"));
    writer.write_event(Event::Empty(clr_map))?;
    Ok(())
}

/// Serializes the single fixed `ppt/slideLayouts/slideLayout1.xml` (`CT_SlideLayout`) this crate
/// ever writes: an empty shape tree, and `<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>`
/// (inherits the master's color map rather than overriding it).
fn to_slide_layout_xml() -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:sldLayout");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    writer.write_event(Event::Start(root))?;

    writer.write_event(Event::Start(BytesStart::new("p:cSld")))?;
    write_shape_tree(
        &mut writer,
        &[],
        &mut PackageState::new(),
        &mut PartRelationships::new(),
    )?;
    writer.write_event(Event::End(BytesEnd::new("p:cSld")))?;

    write_color_map_override(&mut writer)?;

    writer.write_event(Event::End(BytesEnd::new("p:sldLayout")))?;
    Ok(writer.into_inner())
}

/// Serializes `ppt/theme/theme1.xml` (`CT_OfficeStyleSheet`) — `clrScheme`/ `fontScheme` come from
/// `theme` (Office's own built-in default, [`Theme::office_default`], when `None`, matching this
/// function's previous always-fixed behavior byte-for-byte for colors/fonts, modulo one deliberate
/// simplification: `dk1`/`lt1` are now always written as plain `<a:srgbClr>` rather than the live
/// `<a:sysClr>` binding real Word/PowerPoint themes use for those two slots — schema-valid in its
/// place and renders identically outside of that live system-color binding, which this crate has no
/// way to honor anyway; same posture `word-ooxml::Theme`'s own doc comment already documents for
/// itself). `fmtScheme` stays a fixed copy of Office's own built-in default regardless — see
/// [`Theme`]'s own doc comment for why.
fn to_theme_xml(theme: Option<&Theme>) -> Vec<u8> {
    let owned_default;
    let theme = match theme {
        Some(theme) => theme,
        None => {
            owned_default = Theme::office_default();
            &owned_default
        }
    };

    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(&format!(
        r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="{}">"#,
        xml_escape(&theme.name)
    ));
    xml.push_str("<a:themeElements>");
    xml.push_str(&to_color_scheme_xml(&theme.name, &theme.colors));
    xml.push_str(&to_font_scheme_xml(&theme.name, &theme.fonts));
    xml.push_str(FIXED_FMT_SCHEME_XML);
    xml.push_str("</a:themeElements>");
    xml.push_str("</a:theme>");
    xml.into_bytes()
}

/// Serializes `<a:clrScheme name="..">`'s 12 color slots from a [`ColorScheme`] — always as
/// `<a:srgbClr>` (see [`to_theme_xml`]'s own doc comment for why `dk1`/`lt1`'s live `<a:sysClr>`
/// binding isn't modeled).
fn to_color_scheme_xml(name: &str, colors: &ColorScheme) -> String {
    format!(
        concat!(
            r#"<a:clrScheme name="{name}">"#,
            r#"<a:dk1><a:srgbClr val="{dk1}"/></a:dk1>"#,
            r#"<a:lt1><a:srgbClr val="{lt1}"/></a:lt1>"#,
            r#"<a:dk2><a:srgbClr val="{dk2}"/></a:dk2>"#,
            r#"<a:lt2><a:srgbClr val="{lt2}"/></a:lt2>"#,
            r#"<a:accent1><a:srgbClr val="{accent1}"/></a:accent1>"#,
            r#"<a:accent2><a:srgbClr val="{accent2}"/></a:accent2>"#,
            r#"<a:accent3><a:srgbClr val="{accent3}"/></a:accent3>"#,
            r#"<a:accent4><a:srgbClr val="{accent4}"/></a:accent4>"#,
            r#"<a:accent5><a:srgbClr val="{accent5}"/></a:accent5>"#,
            r#"<a:accent6><a:srgbClr val="{accent6}"/></a:accent6>"#,
            r#"<a:hlink><a:srgbClr val="{hlink}"/></a:hlink>"#,
            r#"<a:folHlink><a:srgbClr val="{fol_hlink}"/></a:folHlink>"#,
            "</a:clrScheme>",
        ),
        name = xml_escape(name),
        dk1 = colors.dark1,
        lt1 = colors.light1,
        dk2 = colors.dark2,
        lt2 = colors.light2,
        accent1 = colors.accent1,
        accent2 = colors.accent2,
        accent3 = colors.accent3,
        accent4 = colors.accent4,
        accent5 = colors.accent5,
        accent6 = colors.accent6,
        hlink = colors.hyperlink,
        fol_hlink = colors.followed_hyperlink,
    )
}

/// Serializes `<a:fontScheme name="..">` from a [`FontScheme`] — Latin typeface only, `ea`/`cs`
/// always empty (see `model.rs`'s `FontScheme` doc comment).
fn to_font_scheme_xml(name: &str, fonts: &FontScheme) -> String {
    format!(
        concat!(
            r#"<a:fontScheme name="{name}">"#,
            r#"<a:majorFont><a:latin typeface="{major}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>"#,
            r#"<a:minorFont><a:latin typeface="{minor}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>"#,
            "</a:fontScheme>",
        ),
        name = xml_escape(name),
        major = xml_escape(&fonts.major_latin),
        minor = xml_escape(&fonts.minor_latin),
    )
}

/// Escapes the handful of characters that are unsafe inside an XML attribute value —
/// `theme`/`fonts` come from caller-supplied strings (unlike every other piece of fixed XML this
/// module writes via `concat!`), so, unlike those, they can't just be trusted verbatim.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `<a:fmtScheme>` (`CT_BaseStyles` requires it alongside `clrScheme`/ `fontScheme`) — always
/// Office's own fixed built-in default, not customizable (see [`Theme`]'s own doc comment).
const FIXED_FMT_SCHEME_XML: &str = concat!(
    r#"<a:fmtScheme name="Office">"#,
    "<a:fillStyleLst>",
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="35000"><a:schemeClr val="phClr"><a:tint val="37000"/><a:satMod val="300000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="100000"/><a:shade val="100000"/><a:satMod val="130000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="50000"/><a:shade val="100000"/><a:satMod val="350000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:lin ang="16200000" scaled="0"/></a:gradFill>"#,
    "</a:fillStyleLst>",
    "<a:lnStyleLst>",
    r#"<a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"><a:shade val="95000"/><a:satMod val="105000"/></a:schemeClr></a:solidFill><a:prstDash val="solid"/></a:ln>"#,
    r#"<a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>"#,
    r#"<a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>"#,
    "</a:lnStyleLst>",
    "<a:effectStyleLst>",
    r#"<a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="20000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="38000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>"#,
    r#"<a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="23000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="35000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>"#,
    "<a:effectStyle>",
    r#"<a:effectLst><a:outerShdw blurRad="40000" dist="23000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="35000"/></a:srgbClr></a:outerShdw></a:effectLst>"#,
    r#"<a:scene3d><a:camera prst="orthographicFront"><a:rot lat="0" lon="0" rev="0"/></a:camera><a:lightRig rig="threePt" dir="t"><a:rot lat="0" lon="0" rev="1200000"/></a:lightRig></a:scene3d>"#,
    r#"<a:sp3d><a:bevelT w="63500" h="25400"/></a:sp3d>"#,
    "</a:effectStyle>",
    "</a:effectStyleLst>",
    "<a:bgFillStyleLst>",
    r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="40000"/><a:satMod val="350000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="40000"><a:schemeClr val="phClr"><a:tint val="45000"/><a:shade val="99000"/><a:satMod val="350000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="20000"/><a:satMod val="255000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="-80000" r="50000" b="180000"/></a:path></a:gradFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="80000"/><a:satMod val="300000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="30000"/><a:satMod val="200000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill>"#,
    "</a:bgFillStyleLst>",
    "</a:fmtScheme>",
);

/// Serializes `ppt/presProps.xml` (`CT_PresentationProperties`) — every child is optional
/// (`minOccurs="0"`), so the fixed empty element is schema-valid.
fn to_pres_props_xml() -> Vec<u8> {
    const XML: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:presentationPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#,
    );
    XML.as_bytes().to_vec()
}

/// Serializes `ppt/viewProps.xml` (`CT_ViewProperties`) — same posture as `to_pres_props_xml`:
/// every child is optional.
fn to_view_props_xml() -> Vec<u8> {
    const XML: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:viewPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#,
    );
    XML.as_bytes().to_vec()
}

/// Serializes `ppt/tableStyles.xml` (`CT_TableStyleList`). The root element's `def` attribute
/// (`ST_Guid`, required) stays the well-known "No Style, No Grid" GUID every default Office
/// installation ships (part of the OOXML specification's own reference material, not
/// implementation-specific) regardless of `table_styles` — it only matters as the fallback a table
/// with no explicit `style_id` resolves to (`SlideTable::style_id`'s own doc comment). Each entry
/// in `table_styles` becomes its own `<a:tblStyle>` — see [`crate::model::SlideTableStyle`]'s own
/// doc comment for scope.
fn to_table_styles_xml(table_styles: &[SlideTableStyle]) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("a:tblStyleLst");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("def", TABLE_STYLE_GUID));

    if table_styles.is_empty() {
        writer.write_event(Event::Empty(root))?;
        return Ok(writer.into_inner());
    }

    writer.write_event(Event::Start(root))?;
    for style in table_styles {
        write_table_style(&mut writer, style)?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:tblStyleLst")))?;
    Ok(writer.into_inner())
}

/// Writes one `<a:tblStyle styleId=".." styleName="..">` — only the parts actually set on
/// [`SlideTableStyle`] are written as children, same "only write what was actually set" convention
/// as everywhere else in this crate.
fn write_table_style<W: Write>(writer: &mut Writer<W>, style: &SlideTableStyle) -> Result<()> {
    validate_table_style_id(&style.id)?;
    let mut start = BytesStart::new("a:tblStyle");
    start.push_attribute(("styleId", style.id.as_str()));
    start.push_attribute(("styleName", style.name.as_str()));
    writer.write_event(Event::Start(start))?;

    let parts: [(&str, &Option<TableStylePart>); 9] = [
        ("a:wholeTbl", &style.whole_table),
        ("a:band1H", &style.band1_horizontal),
        ("a:band2H", &style.band2_horizontal),
        ("a:band1V", &style.band1_vertical),
        ("a:band2V", &style.band2_vertical),
        ("a:firstRow", &style.first_row),
        ("a:lastRow", &style.last_row),
        ("a:firstCol", &style.first_column),
        ("a:lastCol", &style.last_column),
    ];
    for (element_name, part) in parts {
        if let Some(part) = part {
            write_table_style_part(writer, element_name, part)?;
        }
    }

    writer.write_event(Event::End(BytesEnd::new("a:tblStyle")))?;
    Ok(())
}

/// Writes one `<a:wholeTbl>{tcTxStyle}{tcStyle}</a:wholeTbl>`-shaped element
/// (`CT_TableStyleTextStyle` + `CT_TableStyleCellStyle`, see [`crate::model::TableStylePart`]'s own
/// doc comment for why this crate combines the two).
fn write_table_style_part<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    part: &TableStylePart,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new(element_name)))?;

    let mut tc_tx_style = BytesStart::new("a:tcTxStyle");
    if part.bold {
        tc_tx_style.push_attribute(("b", "on"));
    }
    if part.italic {
        tc_tx_style.push_attribute(("i", "on"));
    }
    if let Some(color) = &part.text_color {
        writer.write_event(Event::Start(tc_tx_style))?;
        drawing::write_color(writer, color)?;
        writer.write_event(Event::End(BytesEnd::new("a:tcTxStyle")))?;
    } else {
        writer.write_event(Event::Empty(tc_tx_style))?;
    }

    if let Some(fill) = &part.fill {
        writer.write_event(Event::Start(BytesStart::new("a:tcStyle")))?;
        writer.write_event(Event::Start(BytesStart::new("a:fill")))?;
        drawing::write_fill(writer, fill)?;
        writer.write_event(Event::End(BytesEnd::new("a:fill")))?;
        writer.write_event(Event::End(BytesEnd::new("a:tcStyle")))?;
    } else {
        writer.write_event(Event::Empty(BytesStart::new("a:tcStyle")))?;
    }

    writer.write_event(Event::End(BytesEnd::new(element_name)))?;
    Ok(())
}

/// Serializes `docProps/core.xml` from a presentation's [`DocumentProperties`] — a direct port of
/// `excel-ooxml`'s own `to_core_properties_xml` (see that function's own doc comment).
/// Every `CT_CoreProperties` child is optional, so an empty `<cp:coreProperties>` root remains
/// schema-valid when every field is `None`.
fn to_core_properties_xml(properties: &DocumentProperties) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("cp:coreProperties");
    root.push_attribute((
        "xmlns:cp",
        "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
    ));
    root.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    root.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));

    let has_any = properties.title.is_some()
        || properties.author.is_some()
        || properties.subject.is_some()
        || properties.keywords.is_some();
    if !has_any {
        writer.write_event(Event::Empty(root))?;
        return Ok(writer.into_inner());
    }
    writer.write_event(Event::Start(root))?;

    if let Some(title) = &properties.title {
        write_text_element(&mut writer, "dc:title", title)?;
    }
    if let Some(author) = &properties.author {
        write_text_element(&mut writer, "dc:creator", author)?;
    }
    if let Some(subject) = &properties.subject {
        write_text_element(&mut writer, "dc:subject", subject)?;
    }
    if let Some(keywords) = &properties.keywords {
        write_text_element(&mut writer, "cp:keywords", keywords)?;
    }

    writer.write_event(Event::End(BytesEnd::new("cp:coreProperties")))?;
    Ok(writer.into_inner())
}

/// Serializes `docProps/app.xml` (`CT_Properties`) from a presentation's [`DocumentProperties`].
/// `Application` is always set (mirrors `word-ooxml`/`excel-ooxml`'s own extended properties);
/// `Slides` is filled in from the actual slide count since it costs nothing extra and real
/// PowerPoint always sets it; `Company`/`Manager`/`HyperlinkBase` are each set only when the
/// matching `DocumentProperties` field is.
fn to_extended_properties_xml(
    slide_count: usize,
    properties: &DocumentProperties,
) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("Properties");
    root.push_attribute((
        "xmlns",
        "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
    ));
    writer.write_event(Event::Start(root))?;

    write_text_element(&mut writer, "Application", "office-toolkit")?;
    write_text_element(&mut writer, "Slides", slide_count.to_string().as_str())?;
    if let Some(company) = &properties.company {
        write_text_element(&mut writer, "Company", company)?;
    }
    if let Some(manager) = &properties.manager {
        write_text_element(&mut writer, "Manager", manager)?;
    }
    if let Some(hyperlink_base) = &properties.hyperlink_base {
        write_text_element(&mut writer, "HyperlinkBase", hyperlink_base)?;
    }

    writer.write_event(Event::End(BytesEnd::new("Properties")))?;
    Ok(writer.into_inner())
}

/// Serializes `docProps/custom.xml` (OPC custom properties) from a presentation's
/// [`DocumentProperties::custom_properties`] — only called when non-empty. A direct port of
/// `excel-ooxml`'s own `to_custom_properties_xml` — see that function's own doc comment for the
/// `fmtid`/`pid` scheme.
fn to_custom_properties_xml(
    custom_properties: &[(String, CustomPropertyValue)],
) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("Properties");
    root.push_attribute(("xmlns", CUSTOM_PROPERTIES_NAMESPACE));
    root.push_attribute(("xmlns:vt", CUSTOM_PROPERTIES_VT_NAMESPACE));
    writer.write_event(Event::Start(root))?;

    for (index, (name, value)) in custom_properties.iter().enumerate() {
        let pid = FIRST_CUSTOM_PROPERTY_PID + index as i32;

        let mut property = BytesStart::new("property");
        property.push_attribute(("fmtid", CUSTOM_PROPERTY_FMTID));
        property.push_attribute(("pid", pid.to_string().as_str()));
        property.push_attribute(("name", name.as_str()));
        writer.write_event(Event::Start(property))?;

        let (variant_element, text) = match value {
            CustomPropertyValue::Text(text) => ("vt:lpwstr", text.clone()),
            CustomPropertyValue::Bool(value) => (
                "vt:bool",
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
            CustomPropertyValue::Int(value) => ("vt:i4", value.to_string()),
            CustomPropertyValue::Number(value) => ("vt:r8", value.to_string()),
        };
        write_text_element(&mut writer, variant_element, &text)?;

        writer.write_event(Event::End(BytesEnd::new("property")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("Properties")))?;
    Ok(writer.into_inner())
}

/// Writes `<element_name>{text}</element_name>`, a tiny helper used by every docProps writer above
/// to cut down on repetition.
fn write_text_element<W: Write>(
    writer: &mut Writer<W>,
    element_name: &str,
    text: &str,
) -> Result<()> {
    writer.write_event(Event::Start(BytesStart::new(element_name)))?;
    writer.write_event(Event::Text(xml_core::BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(element_name)))?;
    Ok(())
}

/// Serializes the single fixed `ppt/notesMasters/notesMaster1.xml` (`CT_NotesMaster`) this crate
/// ever writes: an empty shape tree, the standard default color map (see
/// [`write_default_color_map`]). `<p:notesStyle>` (default paragraph formatting for notes text) is
/// `minOccurs="0"` and omitted, same "skip optional, out-of-scope boilerplate" posture as the slide
/// master's own `<p:txStyles>`.
fn to_notes_master_xml() -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:notesMaster");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    writer.write_event(Event::Start(root))?;

    writer.write_event(Event::Start(BytesStart::new("p:cSld")))?;
    write_shape_tree(
        &mut writer,
        &[],
        &mut PackageState::new(),
        &mut PartRelationships::new(),
    )?;
    writer.write_event(Event::End(BytesEnd::new("p:cSld")))?;

    write_default_color_map(&mut writer)?;

    writer.write_event(Event::End(BytesEnd::new("p:notesMaster")))?;
    Ok(writer.into_inner())
}

/// Serializes one `ppt/notesSlides/notesSlideN.xml` from a slide's speaker notes text. The root
/// element is `<p:notes>` (`CT_NotesSlide`) — note this differs from the part's own file name
/// convention, confirmed against a real notes-bearing `.pptx` fixture. Real PowerPoint always wraps
/// notes text in two placeholder shapes — a `sldImg` placeholder (the slide thumbnail area, left
/// empty here — this crate doesn't render slide thumbnails) and a `body` placeholder at `idx="1"`
/// (the actual notes text) — reconstructed here the same way, reusing
/// [`write_shape_tree`]/[`write_auto_shape`] directly rather than duplicating their
/// placeholder-writing logic. A third, optional `sldNum` placeholder real PowerPoint also writes is
/// omitted (cosmetic, out of scope).
fn to_notes_slide_xml(notes: &drawing::TextBody) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))?;

    let mut root = BytesStart::new("p:notes");
    root.push_attribute(("xmlns:a", A_NAMESPACE));
    root.push_attribute(("xmlns:r", R_NAMESPACE));
    root.push_attribute(("xmlns:p", P_NAMESPACE));
    writer.write_event(Event::Start(root))?;

    let slide_image_shape = Shape::AutoShape(
        AutoShape::new(2, "Image de la diapositive")
            .with_placeholder(Placeholder::new(PlaceholderKind::SlideImage)),
    );
    let body_shape = Shape::AutoShape(
        AutoShape::new(3, "Texte de la note")
            .with_placeholder(Placeholder::new(PlaceholderKind::Body).with_index(1))
            .with_text_body(notes.clone()),
    );

    writer.write_event(Event::Start(BytesStart::new("p:cSld")))?;
    write_shape_tree(
        &mut writer,
        &[slide_image_shape, body_shape],
        &mut PackageState::new(),
        &mut PartRelationships::new(),
    )?;
    writer.write_event(Event::End(BytesEnd::new("p:cSld")))?;

    write_color_map_override(&mut writer)?;

    writer.write_event(Event::End(BytesEnd::new("p:notes")))?;
    Ok(writer.into_inner())
}

/// Serializes one `ppt/comments/commentN.xml` (`CT_CommentList`) from a slide's [`SlideComment`]s,
/// registering each comment's author against `authors` (assigning a numeric id, deduplicated by
/// name) along the way. **Not grounded on a real fixture** — see `model.rs`'s top-level doc
/// comment.
fn to_comments_xml(comments: &[SlideComment], authors: &mut CommentAuthorRegistry) -> Vec<u8> {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(r#"<p:cmLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#);
    for comment in comments {
        let author_id = authors.id_for(&comment.author, &comment.initials);
        xml.push_str(&format!(
            r#"<p:cm authorId="{author_id}" dt="{dt}" idx="1"><p:pos x="{x}" y="{y}"/><p:text>{text}</p:text></p:cm>"#,
            author_id = author_id,
            dt = xml_escape(&comment.date),
            x = comment.position_emu.0,
            y = comment.position_emu.1,
            text = xml_escape(&comment.text),
        ));
    }
    xml.push_str("</p:cmLst>");
    xml.into_bytes()
}

/// Serializes `ppt/commentAuthors.xml` (`CT_CommentAuthorList`) from the package-wide deduplicated
/// author list collected while writing every slide's own comments (see [`CommentAuthorRegistry`]'s
/// own doc comment). `<p:cmAuthor>`'s `lastIdx`/`clrIdx` attributes are mandatory per
/// `CT_CommentAuthor` — `lastIdx` (the highest comment index this author has authored) is always
/// written as `1` (this crate never assigns a comment any `idx` other than `1`, see
/// [`to_comments_xml`]); `clrIdx` (which of PowerPoint's author-color swatches to use) cycles
/// through 0-3 by registration order, matching real PowerPoint's own round-robin assignment.
fn to_comment_authors_xml(authors: &CommentAuthorRegistry) -> Vec<u8> {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(
        r#"<p:cmAuthorLst xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
    );
    for (id, (name, initials)) in authors.authors.iter().enumerate() {
        xml.push_str(&format!(
            r#"<p:cmAuthor id="{id}" name="{name}" initials="{initials}" lastIdx="1" clrIdx="{clr_idx}"/>"#,
            id = id,
            name = xml_escape(name),
            initials = xml_escape(initials),
            clr_idx = id % 4,
        ));
    }
    xml.push_str("</p:cmAuthorLst>");
    xml.into_bytes()
}
