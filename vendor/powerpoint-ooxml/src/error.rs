//! Error type for `powerpoint-ooxml`.

use thiserror::Error;

/// Errors that can occur while reading or writing a `.pptx` presentation.
#[derive(Debug, Error)]
pub enum Error {
    /// An error occurred at the OPC package level (ZIP container, parts, relationships).
    #[error("OPC package error: {0}")]
    Opc(#[from] opc::Error),

    /// An error occurred while reading or writing XML.
    #[error("XML error: {0}")]
    Xml(#[from] xml_core::Error),

    /// An error occurred while reading or writing a shape's DrawingML content
    /// (fill/line/text/geometry).
    #[error("drawing error: {0}")]
    Drawing(#[from] drawing::Error),

    /// An error occurred while reading or writing an embedded chart's content.
    #[error("chart error: {0}")]
    Chart(#[from] chart::Error),

    /// A part is not valid UTF-8.
    #[error("part '{0}' is not valid UTF-8: {1}")]
    InvalidUtf8(String, std::str::Utf8Error),

    /// The package has no relationship of type `../relationships/officeDocument`, so the main
    /// `ppt/presentation.xml` part could not be located.
    #[error("the package has no main presentation part (missing officeDocument relationship)")]
    MissingMainPresentation,

    /// `ppt/presentation.xml`'s `<p:sldIdLst>` referenced a relationship id (`r:id`) that has no
    /// matching entry in `ppt/_rels/presentation.xml.rels`.
    #[error("slide relationship '{0}' is not declared in ppt/_rels/presentation.xml.rels")]
    MissingSlideRelationship(String),

    /// A `<p:sldId>`'s relationship resolved to a part name that isn't actually present in the
    /// package.
    #[error("slide part '{0}' is missing from the package")]
    MissingSlidePart(String),

    /// A `<p:pic>`'s `<a:blip r:embed="..">` referenced a relationship id that has no matching
    /// entry in the slide's own `.rels` part.
    #[error("image relationship '{0}' is not declared in the slide's own .rels part")]
    MissingImageRelationship(String),

    /// An image relationship resolved to a part name that isn't actually present in the package.
    #[error("image part '{0}' is missing from the package")]
    MissingImagePart(String),

    /// An image part's name doesn't carry a recognized extension
    /// (`.png`/`.jpeg`/`.jpg`/`.gif`/`.bmp`).
    #[error("image part '{0}' has an unsupported or unrecognized format")]
    UnsupportedImageFormat(String),

    /// A `<c:chart r:id="..">` referenced a relationship id that has no matching entry in the
    /// slide's own `.rels` part.
    #[error("chart relationship '{0}' is not declared in the slide's own .rels part")]
    MissingChartRelationship(String),

    /// A chart relationship resolved to a part name that isn't actually present in the package.
    #[error("chart part '{0}' is missing from the package")]
    MissingChartPart(String),

    /// A slide's own `notesSlide` relationship resolved to a part name that isn't actually present
    /// in the package.
    #[error("notes slide part '{0}' is missing from the package")]
    MissingNotesPart(String),

    /// A `<p:pic>`'s `<a:videoFile>`/`<a:audioFile r:link=".">` referenced a relationship id that
    /// has no matching entry in the slide's own `.rels` part.
    #[error("media relationship '{0}' is not declared in the slide's own .rels part")]
    MissingMediaRelationship(String),

    /// A media relationship resolved to a part name that isn't actually present in the package.
    #[error("media part '{0}' is missing from the package")]
    MissingMediaPart(String),

    /// A media part's name doesn't carry a recognized extension
    /// (`.mp4`/`.wmv`/`.avi`/`.mp3`/`.wav`).
    #[error("media part '{0}' has an unsupported or unrecognized format")]
    UnsupportedMediaFormat(String),

    /// A table style id ([`SlideTable::style_id`](crate::model::SlideTable::style_id) or
    /// [`SlideTableStyle::id`](crate::model::SlideTableStyle)) is not a valid `ST_Guid`. ECMA-376
    /// requires both `<a:tblStyle styleId="..">` and `<a:tableStyleId>` to hold a GUID in
    /// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` form (uppercase hex) — anything else serializes to
    /// schema-invalid XML that PowerPoint reports as a damaged file needing repair, with no error
    /// at write time to explain why. Caught here instead, before that XML is ever written.
    #[error(
        "table style id '{0}' is not a valid GUID — OOXML requires the form \
         `{{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}}` (uppercase hex); a non-GUID id \
         writes schema-invalid XML that PowerPoint will refuse to open without repairing"
    )]
    InvalidTableStyleId(String),
}

/// A [`Result`](std::result::Result) alias using [`Error`] as the error type.
pub type Result<T> = std::result::Result<T, Error>;
