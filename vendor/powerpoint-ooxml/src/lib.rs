//! `powerpoint-ooxml` — reading and writing of the PresentationML format (.pptx).
//!
//! The deepest consumer of the shared `drawing` crate — a slide's shapes essentially *are* its
//! content (see `drawing`'s own top-level doc comment, which anticipated exactly this). Builds on
//! `opc` (container/relationships), `xml-core` (low-level XML) and `drawing` (shape
//! geometry/fill/line/text, reused verbatim for a shape's `<p:spPr>`/`<p:txBody>`). All modeling
//! and generation code for PresentationML elements is original (no dependency on any third-party
//! OOXML crate).
//!
//! # Content model
//!
//! A presentation is a sequence of slides; a slide is a sequence of shapes carrying a text body.
//! Real PowerPoint's slide master/layout/theme chain is always present in a written package (a
//! single fixed triplet — this crate has no customizable model for the master/layout, only the
//! theme) but is otherwise invisible to callers: [`Presentation`] only exposes slides.
//!
//! [`Shape`] is an enum covering autoshape, picture, chart, [`ShapeGroup`] (`<p:grpSp>`, nestable),
//! [`Connector`] (`<p:cxnSp>`), [`SlideTable`] (`<p:graphicFrame>`/`<a:tbl>`), and [`SlideMedia`]
//! (`<p:pic>` carrying an `<a:videoFile>`/`<a:audioFile>` reference for embedded video/audio). An
//! autoshape can carry a [`Placeholder`] role, an `is_text_box` flag (`txBox="1"`), and a
//! `hyperlink` (a clickable link on the whole shape, backed by a real external relationship this
//! crate's writer registers and resolves). Character-level hyperlinks
//! (`drawing::Hyperlink` on `drawing::TextRunProperties`) round-trip an *existing* file's own
//! relationship id opaquely, but this crate's writer does not yet register a real relationship for
//! one freshly set by a caller.
//!
//! A [`Slide`] can carry speaker notes (written as a genuinely optional `notesSlide`/`notesMaster`
//! part pair, only when at least one slide actually has notes), [`SlideComment`]s (same "only when
//! actually present" convention, via an optional `comments`/`commentAuthors` part pair), and an
//! overridable background fill (`<p:bg>`, via [`Slide::background`]).
//!
//! [`Presentation::theme`] makes the color/font theme customizable
//! ([`Theme`]/[`ColorScheme`]/[`FontScheme`], mirroring `word_ooxml::Theme` by shape, not by crate
//! dependency); the theme part itself is always written (mandatory per ECMA-376), with
//! [`Theme::office_default`]'s content when unset. [`DocumentProperties`] makes
//! `docProps/core.xml`/`app.xml`/`custom.xml` customizable ([`Presentation::properties`], mirroring
//! `excel-ooxml`'s own type of the same name). [`SlideTableStyle`]/[`TableStylePart`] make custom
//! table styles possible ([`Presentation::table_styles`], [`SlideTable::style_id`]).
//! [`EmbeddedFont`] embeds TrueType/OpenType font data directly in the package
//! (`<p:embeddedFontLst>`, [`Presentation::embedded_fonts`]).
//!
//! Animations/transitions, a fully customizable slide master/layout model, SmartArt, and OLE
//! objects are not modeled.

mod error;
mod model;
mod reader;
mod writer;

pub use error::{Error, Result};
pub use model::{
    AutoShape, ColorScheme, Connector, CustomPropertyValue, DEFAULT_NOTES_HEIGHT_EMU,
    DEFAULT_NOTES_WIDTH_EMU, DEFAULT_SLIDE_HEIGHT_EMU, DEFAULT_SLIDE_WIDTH_EMU, DocumentProperties,
    EMU_PER_INCH, EmbeddedFont, FontScheme, MediaFormat, Picture, PictureFormat, Placeholder,
    PlaceholderKind, Presentation, Shape, ShapeConnection, ShapeGroup, Slide, SlideChart,
    SlideComment, SlideHyperlinkTarget, SlideMedia, SlideTable, SlideTableStyle, TableCell,
    TableCellProperties, TableRow, TableStylePart, Theme,
};
