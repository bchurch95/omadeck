//! In-memory model of a `.pptx` package.
//!
//! A presentation is a sequence of slides; a slide is a sequence of [`Shape`]s, an enum covering
//! every distinct shape kind PresentationML defines: plain autoshapes with no placeholder
//! inheritance, images (`Shape::Picture`), charts (`Shape::Chart`), placeholder metadata on
//! autoshapes, per-slide speaker notes, shape groups (`Shape::Group`), connectors
//! (`Shape::Connector`), tables (`Shape::Table`), per-slide comments ([`SlideComment`]), and a
//! customizable color/font theme ([`Theme`]). A fully customizable slide layout/master model,
//! backgrounds, and SmartArt are not modeled.
//!
//! [`Connector`] and [`SlideComment`] have no real fixture in this project's local corpus to ground
//! their structure against (unlike every other shape kind, which was cross-checked against a real,
//! non-synthetic `.pptx`; see each type's own doc comment). Their shape is instead built directly
//! from ECMA-376's own schema text, the same fallback used whenever no fixture happens to exercise
//! a given property (e.g. `word-ooxml`'s `CT_TcBorders` child sequence). Flagged here for extra
//! scrutiny once opened in a real PowerPoint installation.
//!
//! A shape's content (position/size, outline, fill, text) reuses the `drawing` crate's
//! [`drawing::ShapeProperties`]/[`drawing::TextBody`] directly rather than redefining them — the
//! same shared DrawingML vocabulary already used by `word-ooxml` (`<w:drawing>`) and `excel-ooxml`
//! (`xdr:sp`/`xdr:pic`). See `drawing`'s own top-level doc comment: it explicitly anticipated
//! PowerPoint as the format that would lean on it "the most heavily of the three, since a slide's
//! shapes essentially *are* its content". A chart on a slide reuses the `chart` crate's
//! `ChartSpace` verbatim, same posture as `word-ooxml`/ `excel-ooxml`'s own embedded charts.

use chart::ChartSpace;
use drawing::{Color, Fill, Line, ShapeProperties, TextAnchor, TextBody};

/// EMUs (English Metric Units) per inch — 914,400, the same conversion factor
/// `word-ooxml`/`drawing` already use for every EMU-denominated field.
pub const EMU_PER_INCH: i64 = 914_400;

/// The default slide size this crate uses when a [`Presentation`] doesn't override it: 16:9
/// widescreen, `12192000 x 6858000` EMU (13.333 x 7.5 inches) — real PowerPoint's own current
/// default.
pub const DEFAULT_SLIDE_WIDTH_EMU: i64 = 12_192_000;
pub const DEFAULT_SLIDE_HEIGHT_EMU: i64 = 6_858_000;

/// The default notes page size (4:3 portrait, `6858000 x 9144000` EMU), same posture: not
/// customizable, just written as a fixed, schema-complete value so a generated presentation always
/// has a valid `<p:notesSz>` (mandatory per `CT_Presentation`), matching the real fixture's own
/// value.
pub const DEFAULT_NOTES_WIDTH_EMU: i64 = 6_858_000;
pub const DEFAULT_NOTES_HEIGHT_EMU: i64 = 9_144_000;

/// A `.pptx` presentation: an ordered list of slides, plus the slide size.
///
/// A real PowerPoint package also always carries a slide master, at least one slide layout, and a
/// theme — but this crate has no customizable model for the slide master/layout (still a single
/// minimal, fixed pair under the hood; the reader ignores their content entirely, it only resolves
/// each slide's own shapes). The theme, however, *is* customizable (`theme`) — unlike the
/// master/layout, a slide master's theme relationship is mandatory per ECMA-376
/// but its actual color/font content is the single most visible piece of a deck's visual identity,
/// so it gets its own model rather than staying permanently fixed like `excel-ooxml`'s theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Presentation {
    /// The slides, in display order.
    pub slides: Vec<Slide>,
    /// `<p:sldSz cx=".." cy="..">`, in EMUs.
    pub slide_width_emu: i64,
    pub slide_height_emu: i64,
    /// This presentation's color/font theme. `None` (the default) still always produces a complete
    /// `ppt/theme/theme1.xml` — the part itself is mandatory, not optional, unlike e.g.
    /// `word-ooxml::Document.theme` — just with Office's own built-in default colors/fonts
    /// ([`Theme::office_default`]) rather than a caller-supplied palette.
    pub theme: Option<Theme>,
    /// Document metadata (`docProps/core.xml`/`app.xml`/`custom.xml`). Ports `excel-ooxml`'s own
    /// [`DocumentProperties`] posture unchanged (same fields, same "each `None`/empty stays an
    /// omitted, schema-valid element/part" behavior).
    pub properties: DocumentProperties,
    /// Package-wide custom table styles (`ppt/tableStyles.xml`'s own `<a:tblStyle>` entries, beyond
    /// the single fixed built-in one this crate always declares as `def`). Referenced by id from
    /// [`SlideTable::style_id`]. An empty `Vec` (the default) means every table in this
    /// presentation uses the fixed built-in style, unchanged.
    pub table_styles: Vec<SlideTableStyle>,
    /// TrueType/OpenType fonts embedded directly in the package (`ppt/fonts/fontN.fntdata`,
    /// `<p:embeddedFontLst>`). Lets a presentation carry a non-standard font so it renders
    /// correctly even on a machine that doesn't have that font installed. An empty `Vec` (the
    /// default) omits `<p:embeddedFontLst>` entirely and leaves `<p:presentation>`'s own
    /// `embedTrueTypeFonts` attribute unset, same "only write what was actually set" convention as
    /// every other optional part in this crate.
    pub embedded_fonts: Vec<EmbeddedFont>,
}

impl Presentation {
    /// Creates an empty presentation (no slides), with the default 16:9 widescreen slide size and
    /// no custom theme (Office's own built-in default colors/fonts are used).
    pub fn new() -> Self {
        Self {
            slides: Vec::new(),
            slide_width_emu: DEFAULT_SLIDE_WIDTH_EMU,
            slide_height_emu: DEFAULT_SLIDE_HEIGHT_EMU,
            theme: None,
            properties: DocumentProperties::new(),
            table_styles: Vec::new(),
            embedded_fonts: Vec::new(),
        }
    }

    /// Appends a slide.
    pub fn with_slide(mut self, slide: Slide) -> Self {
        self.slides.push(slide);
        self
    }

    /// Appends an empty slide and returns a mutable reference to it, to build it up in place
    /// (`presentation.add_slide().add_shape(..)`). A `&mut self` counterpart to
    /// [`Self::with_slide`], for building a presentation slide-by-slide in a loop without the
    /// consuming builder's `presentation = presentation.with_slide(..)` reassignment on every
    /// iteration.
    pub fn add_slide(&mut self) -> &mut Slide {
        self.slides.push(Slide::new());
        self.slides.last_mut().expect("a slide was just pushed")
    }

    /// Overrides the slide size (default: 16:9 widescreen).
    pub fn with_slide_size(mut self, width_emu: i64, height_emu: i64) -> Self {
        self.slide_width_emu = width_emu;
        self.slide_height_emu = height_emu;
        self
    }

    /// Sets a custom color/font theme, replacing Office's own built-in default.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Sets this presentation's document metadata.
    pub fn with_properties(mut self, properties: DocumentProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Appends a custom table style.
    pub fn with_table_style(mut self, table_style: SlideTableStyle) -> Self {
        self.table_styles.push(table_style);
        self
    }

    /// Appends an embedded font.
    pub fn with_embedded_font(mut self, font: EmbeddedFont) -> Self {
        self.embedded_fonts.push(font);
        self
    }
}

impl Default for Presentation {
    fn default() -> Self {
        Self::new()
    }
}

/// A single slide: an ordered list of shapes (`<p:spTree>`'s children), plus optional speaker
/// notes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slide {
    pub shapes: Vec<Shape>,
    /// This slide's speaker notes (`ppt/notesSlides/notesSlideN.xml`'s own "body" placeholder
    /// text), if any. `None` (the default) means no notes part is written for this slide at all —
    /// real PowerPoint packages commonly have no notes for most slides, so this stays a genuinely
    /// optional, per-slide part rather than always-present boilerplate (mirrors `word-ooxml`'s
    /// header/footer, `styles.xml`, etc. — every optional part in this workspace follows the same
    /// "only written when actually set" convention). See [`Presentation`]'s writer for the fixed
    /// notes-master/notes-slide boilerplate this produces around the text itself.
    pub notes: Option<TextBody>,
    /// This slide's comments (`ppt/comments/commentN.xml`), if any — a genuinely optional part,
    /// only written when non-empty, same "only written when actually set" convention as `notes`.
    /// See [`SlideComment`]'s own doc comment for the caveat that this feature isn't grounded on a
    /// real fixture.
    pub comments: Vec<SlideComment>,
    /// `<p:cSld>/<p:bg>/<p:bgPr>` — this slide's own background fill, overriding whatever the slide
    /// layout/master would otherwise show through. `None` (the default) omits `<p:bg>` entirely —
    /// the slide simply inherits its background from the layout/master chain, same "no override"
    /// default every other optional field in this crate uses. Only the direct fill choice
    /// (`<p:bgPr>`, `EG_FillProperties`) is modeled — `<p:bgRef>` (a reference into the theme's own
    /// background style list) is not. Grounded against a real fixture (`ppt/slides/slide2.xml`),
    /// which confirmed `<p:bg>` as `<p:cSld>`'s very first child, directly wrapping `<p:bgPr>`
    /// before any fill.
    pub background: Option<Fill>,
    /// `<p:sld show="0">` (`CT_Slide`'s own `show` attribute) — hides this slide from the normal
    /// slide-show sequence (it's still present in the deck, just skipped when presenting;
    /// PowerPoint's own "Hide Slide" toggle). `false` (the default) omits the attribute, matching
    /// the schema's own default of a shown slide. Grounded against a real fixture.
    pub hidden: bool,
    /// `<p:sld name=".">` — an optional display name for this slide (distinct from its title
    /// placeholder's own text), shown e.g. in PowerPoint's Slide Navigator/Outline view. `None`
    /// omits the attribute entirely. **Not grounded on a fixture** — no corpus `.pptx` file
    /// examined actually sets it — modeled directly from `CT_Slide`'s schema anyway, the same
    /// posture already used for e.g. [`crate::model::SlideComment`]/[`crate::model::Connector`].
    pub name: Option<String>,
    /// `<p:sld showMasterSp="0">` (`CT_Slide`'s own `showMasterSp` attribute, inverted here) —
    /// hides this slide's inherited slide- master placeholder graphics/decorations (e.g. a logo or
    /// footer bar drawn directly on the master, not through a placeholder this slide overrides).
    /// The schema's own default is `showMasterSp="1"` (shown) when the attribute is omitted — this
    /// field is named/stored inverted (`false` = shown, matching every other boolean "override"
    /// field in this crate, e.g. `hidden`) so that `Slide::new`'s all-`false` default already
    /// matches real PowerPoint's own default behavior, and the attribute is only ever written (as
    /// `"0"`) when explicitly hidden. Grounded against real fixtures.
    pub hide_master_graphics: bool,
}

impl Slide {
    /// Creates an empty slide (no shapes, no notes, no comments, no background override, shown,
    /// unnamed, showing inherited master graphics).
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a shape.
    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shapes.push(shape);
        self
    }

    /// Appends a shape and returns a mutable reference to it. A `&mut self` counterpart to
    /// [`Self::with_shape`], same rationale as [`Presentation::add_slide`].
    pub fn add_shape(&mut self, shape: Shape) -> &mut Shape {
        self.shapes.push(shape);
        self.shapes.last_mut().expect("a shape was just pushed")
    }

    /// Hides this slide from the normal slide-show sequence (`show="0"`).
    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Sets this slide's display name (`<p:sld name="..">`).
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets this slide's speaker notes.
    pub fn with_notes(mut self, notes: TextBody) -> Self {
        self.notes = Some(notes);
        self
    }

    /// Appends a comment.
    pub fn with_comment(mut self, comment: SlideComment) -> Self {
        self.comments.push(comment);
        self
    }

    /// Sets this slide's own background fill, overriding the layout/master.
    pub fn with_background(mut self, background: Fill) -> Self {
        self.background = Some(background);
        self
    }

    /// Hides this slide's inherited slide-master graphics (`showMasterSp="0"`).
    pub fn with_hide_master_graphics(mut self, hide: bool) -> Self {
        self.hide_master_graphics = hide;
        self
    }
}

/// A single item in a slide's shape tree (`<p:spTree>`'s children, in document order) — an enum
/// covering every distinct shape kind PresentationML defines.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// `<p:sp>`, `CT_Shape` — an autoshape or text box, possibly a placeholder.
    AutoShape(AutoShape),
    /// `<p:pic>`, `CT_Picture` — an embedded image.
    Picture(Picture),
    /// `<p:graphicFrame>` wrapping a `<c:chart>` — an embedded chart. Boxed: a chart's own nested
    /// `ChartSpace` dwarfs every other variant here, and clippy's `large_enum_variant` flags the
    /// resulting gap (every `Shape`, no matter its real kind, otherwise pays for the biggest one).
    Chart(Box<SlideChart>),
    /// `<p:grpSp>`, `CT_GroupShape` — a group of shapes moving/resizing as one, itself containing
    /// any of these same variants (including nested groups).
    Group(ShapeGroup),
    /// `<p:cxnSp>`, `CT_Connector` — a straight/elbow/curved connector line, typically linking two
    /// other shapes.
    Connector(Connector),
    /// `<p:graphicFrame>` wrapping an `<a:tbl>` — an embedded table.
    Table(SlideTable),
    /// `<p:pic>` carrying an `<a:videoFile>`/`<a:audioFile>` reference instead of a plain image —
    /// an embedded video or audio clip.
    Media(SlideMedia),
    /// `<p:graphicFrame>` whose `<a:graphicData uri="..">` carries the SmartArt diagram namespace
    /// (`<dgm:relIds r:dm=".." r:lo=".." r:qs=".." r:cs=".."/>` instead of a `<c:chart>`). The
    /// diagram's own data parts (`diagramData1.xml`/`diagramLayout1.xml`/...) are not modeled —
    /// they describe a *layout algorithm*, not fixed slide content, so this variant carries the
    /// frame's identity and geometry only, as an opaque placeholder consumers can position but not
    /// render.
    SmartArt(SmartArt),
}

/// An autoshape or text box (`<p:sp>`, `CT_Shape`).
///
/// Real PowerPoint title/content slides overwhelmingly use *placeholder* shapes
/// (position/formatting inherited from the slide layout/master, see [`Placeholder`]) rather than
/// freestanding ones. `placeholder` is `None` for a freestanding shape and `Some` for one that
/// plays a placeholder role; either way, `properties`/`text_body` describe the shape's own content
/// exactly the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoShape {
    /// `<p:cNvPr id="..">` — must be unique within the slide, and must not collide with `1`, always
    /// reserved by this crate's writer for the slide's own group shape (`<p:nvGrpSpPr><p:cNvPr
    /// id="1"../>`) — mirrors `word-ooxml`'s existing convention of caller-managed numeric ids (see
    /// e.g. `Bookmark::new(id)`/`NoteReference::new(id)`).
    pub id: u32,
    /// `<p:cNvPr name="..">` — a human-readable shape name (e.g. "Zone de texte 1"), purely
    /// cosmetic (shown in PowerPoint's Selection Pane).
    pub name: String,
    /// `<p:spPr>`'s content — position/size, outline geometry, fill, line. Reuses
    /// `drawing::ShapeProperties` verbatim (see this module's own doc comment). When `transform` is
    /// unset, the writer still always emits a fixed zero-valued `<a:xfrm>` and a default `rect`
    /// `<a:prstGeom>` — the same "always write geometry, never delegate silently" fix this project
    /// already had to apply twice to `excel-ooxml`'s picture writing (missing geometry, then
    /// missing `<a:xfrm>`, both making a shape invisible in real Excel) — applied here proactively
    /// rather than rediscovered.
    ///
    /// Unlike `excel-ooxml`'s picture reader (which unconditionally clears `transform`/demotes a
    /// plain `rect` `geometry` back to `None` on read, because a picture's *real* position always
    /// comes from its separate two-cell anchor, never from `spPr`'s own `xfrm`), this crate's
    /// reader makes no such attempt for a *freestanding* shape: `xfrm` is its only source of
    /// position/size, so a real, non-zero transform must round-trip faithfully. This means an
    /// [`AutoShape`] written with `properties` left at its `ShapeProperties::new()` default reads
    /// back with an explicit zero-offset/zero-extent transform and a `rect` geometry, not `None` —
    /// a known, deliberate round-trip asymmetry for this specific case, not a bug. A *placeholder*
    /// shape (`<p:spPr/>` empty, position/formatting inherited) reads back with `properties` at
    /// that same default, for the exact same textual reason — genuinely indistinguishable from the
    /// freestanding zero-sized case by this crate, another accepted limitation of not modeling
    /// slide layout inheritance yet.
    pub properties: ShapeProperties,
    /// `<p:txBody>` — this shape's text content, if any. Reuses `drawing::TextBody` verbatim.
    pub text_body: Option<TextBody>,
    /// `<p:nvSpPr>/<p:nvPr>/<p:ph>` — this shape's placeholder role, if any. `None` for a
    /// freestanding shape.
    pub placeholder: Option<Placeholder>,
    /// `<p:nvSpPr>/<p:cNvSpPr txBox="1">` — marks this shape as a genuine text box rather than an
    /// ordinary autoshape. Purely a hint real PowerPoint itself uses (e.g. to decide whether the
    /// shape gets a default outline/fill when the user starts typing into an empty one) — this
    /// crate doesn't otherwise treat a text box specially; `properties`/`text_body` describe its
    /// content exactly the same way regardless. Grounded against a real fixture confirming the
    /// exact `<p:cNvSpPr txBox="1"/>` shape.
    pub is_text_box: bool,
    /// `<p:nvSpPr>/<p:cNvPr>/<a:hlinkClick>` (including the internal-target/relative-jump
    /// variants) — a clickable hyperlink on this whole shape (as opposed to a hyperlink on an
    /// individual text run, `drawing::Hyperlink` on `drawing::TextRunProperties`, which this crate
    /// does reuse for run text but doesn't yet resolve to a real relationship — see `writer.rs`'s
    /// own doc comment on hyperlinks for why shape-level support came first). `None` for no link.
    /// See [`SlideHyperlinkTarget`]'s own doc comment for the target kinds.
    pub hyperlink: Option<SlideHyperlinkTarget>,
}

impl AutoShape {
    /// Creates a freestanding shape with no explicit position/size/fill/ line, no text, no
    /// placeholder role, not a text box, and no hyperlink (use [`AutoShape::with_properties`]/
    /// [`AutoShape::with_text_body`]/[`AutoShape::with_placeholder`]/
    /// [`AutoShape::with_text_box`]/[`AutoShape::with_hyperlink`]/
    /// [`AutoShape::with_hyperlink_target`] to fill those in).
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            properties: ShapeProperties::new(),
            text_body: None,
            placeholder: None,
            is_text_box: false,
            hyperlink: None,
        }
    }

    /// Sets this shape's position/size/outline/fill/line.
    pub fn with_properties(mut self, properties: ShapeProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Sets this shape's text content.
    pub fn with_text_body(mut self, text_body: TextBody) -> Self {
        self.text_body = Some(text_body);
        self
    }

    /// Marks this shape as playing the given placeholder role.
    pub fn with_placeholder(mut self, placeholder: Placeholder) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    /// Marks (or unmarks) this shape as a genuine text box (`txBox="1"`).
    pub fn with_text_box(mut self, is_text_box: bool) -> Self {
        self.is_text_box = is_text_box;
        self
    }

    /// Attaches a clickable hyperlink to this whole shape, given its target URL — sugar for
    /// `with_hyperlink_target(SlideHyperlinkTarget::External(url))`.
    pub fn with_hyperlink(mut self, url: impl Into<String>) -> Self {
        self.hyperlink = Some(SlideHyperlinkTarget::External(url.into()));
        self
    }

    /// Attaches a clickable hyperlink to this whole shape, given any [`SlideHyperlinkTarget`] — the
    /// general form of [`AutoShape::with_hyperlink`], also covering internal slide jumps.
    pub fn with_hyperlink_target(mut self, target: SlideHyperlinkTarget) -> Self {
        self.hyperlink = Some(target);
        self
    }
}

/// A clickable hyperlink's target (`<a:hlinkClick>`, `CT_Hyperlink`): either
/// [`SlideHyperlinkTarget::External`], or a jump to another slide in the *same* presentation rather
/// than an outside URL. `<a:hlinkClick r:id=".">`'s `r:id` resolves to a relationship — for
/// [`SlideHyperlinkTarget::External`] that relationship's target is the URL itself
/// (`TargetMode="External"`); for
/// [`SlideHyperlinkTarget::Slide`] it instead targets another part *inside* the same package
/// (`TargetMode="Internal"`, `"./slides/slideN.xml"`), paired with
/// `action="ppaction://hlinksldjump"`. The four relative-jump variants need no relationship at all
/// — they're expressed purely via `action="ppaction://hlinkshowjump?jump=."` with no `r:id`
/// attribute. Grounded against a real fixture for the `r:id`+`hlinksldjump` shape
/// ([`SlideHyperlinkTarget::Slide`]); the relative- jump actions have no fixture in this project's
/// corpus but are modeled directly from the `ppaction://hlinkshowjump` action values ECMA-376
/// itself documents for "next slide"/"previous slide"/"first slide"/"last slide".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlideHyperlinkTarget {
    /// An ordinary outside URL (e.g. `"https://example.com"`), the only kind this crate supported
    /// previously.
    External(String),
    /// Jumps to another slide in the same presentation, given as a 0-based index into
    /// [`Presentation::slides`] — resolved to a relative relationship target
    /// (`"./slides/slide{index + 1}.xml"`) purely from that index at write time, since slide
    /// numbering is fully deterministic (no pre-built global slide-id map needed). Not validated
    /// against the presentation's actual slide count — an out-of-range index simply produces a link
    /// real PowerPoint would show as broken, the same best-effort posture already used elsewhere in
    /// this crate (e.g. table cell merge spans).
    Slide(usize),
    /// Jumps to the next slide in presentation order.
    NextSlide,
    /// Jumps to the previous slide in presentation order.
    PreviousSlide,
    /// Jumps to the first slide in the presentation.
    FirstSlide,
    /// Jumps to the last slide in the presentation.
    LastSlide,
}

/// `<p:nvPr><p:ph type=".." idx=".."/></p:nvPr>` (`CT_Placeholder`) — a shape's placeholder role:
/// which slot of the slide layout/master it fills, so its position/formatting can be inherited from
/// there instead of being set explicitly. This crate never resolves that inheritance (no slide
/// layout/master model beyond a single fixed, empty one — see `Presentation`'s doc comment);
/// recording `Placeholder` on a read-back shape is purely informational; nothing else in this crate
/// consults it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    /// `type` — which slot this shape fills (`ST_PlaceholderType`).
    pub kind: PlaceholderKind,
    /// `idx` — disambiguates multiple placeholders of the same `kind` on one layout (e.g. several
    /// `body` placeholders). `None` omits the attribute, matching a title placeholder's own
    /// convention (real PowerPoint never writes `idx` on `title`/`ctrTitle`).
    pub index: Option<u32>,
}

impl Placeholder {
    /// Creates a placeholder role with no explicit index.
    pub fn new(kind: PlaceholderKind) -> Self {
        Self { kind, index: None }
    }

    /// Sets this placeholder's `idx` attribute.
    pub fn with_index(mut self, index: u32) -> Self {
        self.index = Some(index);
        self
    }
}

/// A useful subset of `ST_PlaceholderType`'s values — the ones a shape (as opposed to a placeholder
/// *picture*/*chart*/*table*, not modeled) can play. [`PlaceholderKind::Other`] preserves
/// round-trip fidelity for any type not explicitly modeled here (`chart`/`tbl`/
/// `clipArt`/`dgm`/`media`/`sldImg`/`pic`, all placeholder roles for a non-`<p:sp>` shape kind that
/// this crate doesn't tag with placeholder info on the writing side, but must still be able to read
/// back without error), same "closed enum + escape hatch" posture `drawing::PresetShape` already
/// established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceholderKind {
    Title,
    Body,
    CenterTitle,
    SubTitle,
    DateTime,
    SlideNumber,
    Footer,
    Header,
    Object,
    /// `sldImg` — the slide-thumbnail placeholder on a notes slide (see `Presentation`'s writer,
    /// which emits one automatically around `Slide.notes`).
    SlideImage,
    Other(String),
}

impl PlaceholderKind {
    pub(crate) fn xml_token(&self) -> &str {
        match self {
            PlaceholderKind::Title => "title",
            PlaceholderKind::Body => "body",
            PlaceholderKind::CenterTitle => "ctrTitle",
            PlaceholderKind::SubTitle => "subTitle",
            PlaceholderKind::DateTime => "dt",
            PlaceholderKind::SlideNumber => "sldNum",
            PlaceholderKind::Footer => "ftr",
            PlaceholderKind::Header => "hdr",
            PlaceholderKind::Object => "obj",
            PlaceholderKind::SlideImage => "sldImg",
            PlaceholderKind::Other(token) => token,
        }
    }

    pub(crate) fn from_xml_token(token: &str) -> Self {
        match token {
            "title" => PlaceholderKind::Title,
            "body" => PlaceholderKind::Body,
            "ctrTitle" => PlaceholderKind::CenterTitle,
            "subTitle" => PlaceholderKind::SubTitle,
            "dt" => PlaceholderKind::DateTime,
            "sldNum" => PlaceholderKind::SlideNumber,
            "ftr" => PlaceholderKind::Footer,
            "hdr" => PlaceholderKind::Header,
            "obj" => PlaceholderKind::Object,
            "sldImg" => PlaceholderKind::SlideImage,
            other => PlaceholderKind::Other(other.to_string()),
        }
    }
}

/// An embedded image (`<p:pic>`, `CT_Picture`).
///
/// Unlike an [`AutoShape`], a picture's position/size is never optional or defaulted:
/// `offset_emu`/`extent_emu` are plain fields (not routed through `drawing::Transform2D`'s `Option`
/// flexibility), since a PresentationML picture's `<a:xfrm>` — unlike Excel's cell-anchored
/// pictures — is the shape's *only* source of position/size; there is no separate anchor to fall
/// back on, so demanding a concrete value upfront avoids ever having to invent one.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// The raw, encoded image bytes (e.g. the bytes of a `.png` file).
    pub data: Vec<u8>,
    /// The image's encoding, also used to pick the media part's extension and content type.
    pub format: PictureFormat,
    /// `<a:off x=".." y="..">`, in EMUs.
    pub offset_emu: (i64, i64),
    /// `<a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
    /// Alt text / description (`p:cNvPr`'s `descr` attribute). May be empty.
    pub description: String,
    /// Additional shape formatting (fill/line/rotation, and a non-default outline geometry — e.g.
    /// an oval crop) for the picture's own `p:spPr`, beyond the fixed default rectangle. `None`
    /// (the default) produces a plain rectangular picture with no extra fill/line, the same fixed
    /// shape `word-ooxml`/`excel-ooxml`'s own pictures default to. `transform` is never consulted
    /// here — `offset_emu`/`extent_emu` above are always authoritative for a picture's
    /// position/size, mirroring `word_ooxml::Image::shape_properties`'s own posture.
    pub shape_properties: Option<ShapeProperties>,
    /// `<a:blip r:link=".">`'s own external target. When set, the writer emits *both* `r:embed`
    /// (pointing at `data` below, embedded in the package as usual) and `r:link`
    /// (`TargetMode="External"`, this URL/path) on the same `<a:blip>` — PowerPoint's own "embedded
    /// preview cache + externally-linked original" dual pattern, so the picture still displays even
    /// when the external source is unreachable. `None` (the default) omits `r:link` entirely — a
    /// plain, fully-embedded picture, unchanged. Grounded against a real fixture.
    pub external_link: Option<String>,
}

impl Picture {
    /// Creates a picture at the given size, positioned at `(0, 0)` (use [`Picture::with_offset`] to
    /// place it elsewhere), with no alt text, no extra shape formatting, and no external link.
    pub fn new(
        id: u32,
        name: impl Into<String>,
        data: impl Into<Vec<u8>>,
        format: PictureFormat,
        width_emu: i64,
        height_emu: i64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            data: data.into(),
            format,
            offset_emu: (0, 0),
            extent_emu: (width_emu, height_emu),
            description: String::new(),
            shape_properties: None,
            external_link: None,
        }
    }

    /// Sets this picture's position.
    pub fn with_offset(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.offset_emu = (x_emu, y_emu);
        self
    }

    /// Sets this picture's alt text / description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Sets this picture's additional shape formatting (fill/line/ geometry), beyond the fixed
    /// default rectangle.
    pub fn with_shape_properties(mut self, shape_properties: ShapeProperties) -> Self {
        self.shape_properties = Some(shape_properties);
        self
    }

    /// Additionally links this picture to an external image (`r:link`), alongside its own embedded
    /// `data` — PowerPoint's own "embedded preview cache + externally-linked original" dual
    /// pattern.
    pub fn with_external_link(mut self, url: impl Into<String>) -> Self {
        self.external_link = Some(url.into());
        self
    }
}

/// An image's encoding — mirrors `word-ooxml::ImageFormat` by name and scope for cross-crate
/// consistency (same real-world concept, each host crate keeps its own tiny copy rather than one
/// depending on the other — there is no shared "media" crate, and this enum is far too small to
/// justify creating one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
}

impl PictureFormat {
    pub(crate) fn extension(&self) -> &'static str {
        match self {
            PictureFormat::Png => "png",
            PictureFormat::Jpeg => "jpeg",
            PictureFormat::Gif => "gif",
            PictureFormat::Bmp => "bmp",
        }
    }

    pub(crate) fn content_type(&self) -> &'static str {
        match self {
            PictureFormat::Png => "image/png",
            PictureFormat::Jpeg => "image/jpeg",
            PictureFormat::Gif => "image/gif",
            PictureFormat::Bmp => "image/bmp",
        }
    }

    /// Guesses a format from a part name's extension (e.g. `/ppt/media/image1.jpeg"`), for reading
    /// an existing package back.
    pub(crate) fn from_extension(part_name: &str) -> Option<Self> {
        let extension = part_name.rsplit('.').next()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(PictureFormat::Png),
            "jpeg" | "jpg" => Some(PictureFormat::Jpeg),
            "gif" => Some(PictureFormat::Gif),
            "bmp" => Some(PictureFormat::Bmp),
            _ => None,
        }
    }
}

/// An embedded video or audio clip (`<p:pic>` carrying an `<a:videoFile r:link=".">`/`<a:audioFile
/// r:link=".">` reference in its own `<p:nvPr>`, instead of the plain `<a:blip>` image an ordinary
/// [`Picture`] has).
///
/// Real PowerPoint (2010+) also writes a Microsoft-proprietary `<p:extLst><p:ext><p14:media
/// r:embed=".."/></p:ext></p:extLst>` alongside the core `<a:videoFile>`/`<a:audioFile>` element,
/// referencing the same media part a second time for its own newer UI features — not modeled here
/// (an extension outside ECMA-376 itself, same "skip Microsoft-only extensions" posture this crate
/// already takes for `<a:extLst>` generally); the core relationship alone is schema-complete and
/// should still play in real PowerPoint.
///
/// `<p:timing>` (autoplay/on-click trigger, bookmarks, trim points) lives at the *slide* level, not
/// on the shape itself, and is a materially larger, separate feature — not modeled. Unlike
/// [`Picture`], this type has no `shape_properties` field: the writer always produces a plain
/// default rectangle (`<a:prstGeom prst="rect">`, no custom fill/ line), and any non-default
/// geometry/fill/line an existing file's own `<p:pic>` might carry is simply discarded when read
/// back — a video/audio clip's own outline styling is a rare, cosmetic refinement didn't ask for.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideMedia {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// The raw, encoded media bytes (e.g. the bytes of a `.mp4` file).
    pub data: Vec<u8>,
    /// The media's encoding, also used to pick the media part's extension/ content type and to tell
    /// video from audio (`format.is_video()`).
    pub format: MediaFormat,
    /// `<a:off x=".." y="..">`, in EMUs — same "never optional, no separate anchor" posture as
    /// [`Picture::offset_emu`].
    pub offset_emu: (i64, i64),
    /// `<a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
    /// This clip's poster frame (`<p:blipFill>`'s own `<a:blip>`, the still image real PowerPoint
    /// shows before playback) — the raw image bytes plus its format. `None` omits `<a:blip>` from
    /// `<p:blipFill>` entirely (schema-valid — `blip` is `minOccurs="0"` — but the clip will render
    /// with no visible thumbnail until played).
    pub poster_image: Option<(Vec<u8>, PictureFormat)>,
}

impl SlideMedia {
    /// Creates a media clip at the given size, positioned at `(0, 0)`, with no poster frame.
    pub fn new(
        id: u32,
        name: impl Into<String>,
        data: impl Into<Vec<u8>>,
        format: MediaFormat,
        width_emu: i64,
        height_emu: i64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            data: data.into(),
            format,
            offset_emu: (0, 0),
            extent_emu: (width_emu, height_emu),
            poster_image: None,
        }
    }

    /// Sets this clip's position.
    pub fn with_offset(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.offset_emu = (x_emu, y_emu);
        self
    }

    /// Sets this clip's poster frame image.
    pub fn with_poster_image(mut self, data: impl Into<Vec<u8>>, format: PictureFormat) -> Self {
        self.poster_image = Some((data.into(), format));
        self
    }
}

/// A video/audio clip's encoding (`<a:videoFile>`/`<a:audioFile>`'s referenced media part) —
/// mirrors [`PictureFormat`]'s own "closed enum, small known set" posture, covering the handful of
/// container formats real PowerPoint itself natively embeds rather than every format a media player
/// could theoretically open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFormat {
    Mp4,
    Wmv,
    Avi,
    Mp3,
    Wav,
}

impl MediaFormat {
    /// Whether this format is a video (as opposed to an audio-only) clip — decides whether the
    /// writer emits `<a:videoFile>` or `<a:audioFile>`.
    pub(crate) fn is_video(&self) -> bool {
        matches!(self, MediaFormat::Mp4 | MediaFormat::Wmv | MediaFormat::Avi)
    }

    pub(crate) fn extension(&self) -> &'static str {
        match self {
            MediaFormat::Mp4 => "mp4",
            MediaFormat::Wmv => "wmv",
            MediaFormat::Avi => "avi",
            MediaFormat::Mp3 => "mp3",
            MediaFormat::Wav => "wav",
        }
    }

    pub(crate) fn content_type(&self) -> &'static str {
        match self {
            MediaFormat::Mp4 => "video/mp4",
            MediaFormat::Wmv => "video/x-ms-wmv",
            MediaFormat::Avi => "video/avi",
            MediaFormat::Mp3 => "audio/mpeg",
            MediaFormat::Wav => "audio/wav",
        }
    }

    /// Guesses a format from a part name's extension (e.g. `/ppt/media/media1.mp4`), for reading an
    /// existing package back.
    pub(crate) fn from_extension(part_name: &str) -> Option<Self> {
        let extension = part_name.rsplit('.').next()?.to_ascii_lowercase();
        match extension.as_str() {
            "mp4" => Some(MediaFormat::Mp4),
            "wmv" => Some(MediaFormat::Wmv),
            "avi" => Some(MediaFormat::Avi),
            "mp3" => Some(MediaFormat::Mp3),
            "wav" => Some(MediaFormat::Wav),
            _ => None,
        }
    }
}

/// An embedded chart (`<p:graphicFrame>` wrapping a `<c:chart r:id="..">`).
///
/// Reuses the `chart` crate's `ChartSpace` verbatim, same posture as
/// `word-ooxml::EmbeddedChart`/`excel-ooxml::SheetChart` — **cached values only, no embedded
/// `.xlsx` workbook as a data source**, matching the scope of those two.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideChart {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    pub chart_space: ChartSpace,
    /// `<p:xfrm><a:off x=".." y="..">`, in EMUs. Like [`Picture`], a chart's position/size is never
    /// optional — `<p:xfrm>` is its only source of it (no separate anchor, unlike Excel's
    /// `SheetChart`).
    pub offset_emu: (i64, i64),
    /// `<p:xfrm><a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
}

impl SlideChart {
    /// Creates an embedded chart at the given size, positioned at `(0, 0)` (use
    /// [`SlideChart::with_offset`] to place it elsewhere).
    pub fn new(
        id: u32,
        name: impl Into<String>,
        chart_space: ChartSpace,
        width_emu: i64,
        height_emu: i64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            chart_space,
            offset_emu: (0, 0),
            extent_emu: (width_emu, height_emu),
        }
    }

    /// Sets this chart's position.
    pub fn with_offset(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.offset_emu = (x_emu, y_emu);
        self
    }
}

/// A SmartArt diagram frame (`<p:graphicFrame>` whose `<a:graphicData>` carries the
/// `http://schemas.openxmlformats.org/drawingml/2006/diagram` namespace). Unlike a chart — whose
/// `<c:chart r:id>` points at one part containing the actual data — a SmartArt frame points at
/// several parts (`diagramData1.xml`, `diagramLayout1.xml`, `diagramStyle1.xml`,
/// `quickStyle1.xml`, `colors1.xml`) that together encode a *layout algorithm* plus input text,
/// and whose rendered output depends on the viewer's layout engine. This crate models none of
/// that: the struct carries only the frame's own identity and geometry, exactly enough for a
/// consumer to know *something is there and where it sits*.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartArt {
    /// `<p:cNvPr id="..">` — same uniqueness rules as [`AutoShape::id`].
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// `<p:xfrm><a:off x=".." y="..">`, in EMUs.
    pub offset_emu: (i64, i64),
    /// `<p:xfrm><a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
}

impl SmartArt {
    /// Creates a SmartArt diagram frame of the given size at `(0, 0)` (use
    /// [`SmartArt::with_offset`] to place it elsewhere).
    pub fn new(id: u32, name: impl Into<String>, width_emu: i64, height_emu: i64) -> Self {
        Self {
            id,
            name: name.into(),
            offset_emu: (0, 0),
            extent_emu: (width_emu, height_emu),
        }
    }

    /// Sets this SmartArt frame's position.
    pub fn with_offset(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.offset_emu = (x_emu, y_emu);
        self
    }
}

/// A group of shapes moving/resizing together (`<p:grpSp>`, `CT_GroupShape`).
///
/// A group's own transform is two coordinate pairs working together, not one:
/// `offset_emu`/`extent_emu` (`<a:off>`/`<a:ext>`) place the group on the slide, exactly like any
/// other shape; `child_offset_emu`/ `child_extent_emu` (`<a:chOff>`/`<a:chExt>`) define the
/// coordinate space the *children*'s own `xfrm` values are expressed in — real PowerPoint scales
/// between the two automatically whenever they differ (e.g. a group's children keep their original
/// small-scale coordinates while the group itself is stretched much larger on the slide). Confirmed
/// against a real fixture with nested groups, including a group whose child coordinate space
/// genuinely differs from its own on-slide extent. Groups can nest (a `Shape::Group` inside
/// another's own `shapes`), confirmed on the same fixture.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeGroup {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// `<a:xfrm><a:off x=".." y="..">`, in EMUs.
    pub offset_emu: (i64, i64),
    /// `<a:xfrm><a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
    /// `<a:xfrm><a:chOff x=".." y="..">`, in EMUs — the coordinate space this group's children's
    /// own `xfrm` values are expressed in.
    pub child_offset_emu: (i64, i64),
    /// `<a:xfrm><a:chExt cx=".." cy="..">`, in EMUs.
    pub child_extent_emu: (i64, i64),
    /// `<a:xfrm rot=".">` — `ST_Angle`, in 60,000ths of a degree, positive clockwise. `0` (the
    /// default) omits the attribute — no rotation. Symmetric to
    /// [`drawing::Transform2D::rotation_60000ths`]. Grounded against real fixtures.
    pub rotation_60000ths: i32,
    /// `<a:xfrm flipH=".">` — mirrors the whole group horizontally before rotation is applied.
    /// **Not grounded on a group specifically** in the fixture corpus — modeled by analogy to
    /// [`drawing::Transform2D::flip_horizontal`] (identical `CT_GroupTransform2D`/`CT_Transform2D`
    /// attribute, already grounded there).
    pub flip_horizontal: bool,
    /// `<a:xfrm flipV="..">` — mirrors the whole group vertically. Same grounding caveat as
    /// `flip_horizontal`.
    pub flip_vertical: bool,
    /// The group's children, in document order — any shape kind, including nested groups.
    pub shapes: Vec<Shape>,
}

impl ShapeGroup {
    /// Creates an empty group (no children) with a zero-valued transform — use
    /// [`ShapeGroup::with_transform`] to position it and its children's coordinate space, and
    /// [`ShapeGroup::with_shape`] to add children.
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            offset_emu: (0, 0),
            extent_emu: (0, 0),
            child_offset_emu: (0, 0),
            child_extent_emu: (0, 0),
            rotation_60000ths: 0,
            flip_horizontal: false,
            flip_vertical: false,
            shapes: Vec::new(),
        }
    }

    /// Sets this group's own on-slide position/size and its children's coordinate space (often the
    /// same values, when no scaling between the two is needed).
    pub fn with_transform(
        mut self,
        offset_emu: (i64, i64),
        extent_emu: (i64, i64),
        child_offset_emu: (i64, i64),
        child_extent_emu: (i64, i64),
    ) -> Self {
        self.offset_emu = offset_emu;
        self.extent_emu = extent_emu;
        self.child_offset_emu = child_offset_emu;
        self.child_extent_emu = child_extent_emu;
        self
    }

    /// Sets this group's rotation, given in ordinary degrees (converted to the schema's
    /// 60,000ths-of-a-degree unit internally).
    pub fn with_rotation_degrees(mut self, degrees: f64) -> Self {
        self.rotation_60000ths = (degrees * 60_000.0).round() as i32;
        self
    }

    /// Mirrors the whole group horizontally.
    pub fn with_flip_horizontal(mut self, flip: bool) -> Self {
        self.flip_horizontal = flip;
        self
    }

    /// Mirrors the whole group vertically.
    pub fn with_flip_vertical(mut self, flip: bool) -> Self {
        self.flip_vertical = flip;
        self
    }

    /// Appends a child shape.
    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shapes.push(shape);
        self
    }
}

/// A straight/elbow/curved connector line (`<p:cxnSp>`, `CT_Connector`), typically used to visually
/// link two other shapes.
///
/// **Not grounded on a real fixture** — see this module's own top-level doc comment. Structure
/// follows ECMA-376's `CT_Connector`/ `CT_ConnectorNonVisual` directly: a connector's own
/// `<p:spPr>` is the exact same `CT_ShapeProperties` content type an autoshape's `<p:spPr>` uses
/// (position/geometry/fill/line), so `properties` reuses `drawing::ShapeProperties` verbatim, same
/// as [`AutoShape`] — only the non-visual wrapper element name (`p:cxnSp`/`p:nvCxnSpPr` instead of
/// `p:sp`/`p:nvSpPr`) and the optional start/end shape attachments differ. When
/// `properties.geometry` is unset, the writer defaults to `<a:prstGeom prst="line">` (a straight
/// connector) rather than `AutoShape`'s own `rect` default — the same "always write geometry
/// explicitly" discipline, just with a connector-appropriate default preset.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// `<p:spPr>`'s content — position/size, outline geometry, fill, line.
    pub properties: ShapeProperties,
    /// `<p:nvCxnSpPr><p:cNvCxnSpPr><a:stCxn id=".." idx=".."/>` — which other shape (and which of
    /// its connection sites) this connector's start point is attached to. `None` for a floating,
    /// unattached start point (positioned by `properties`' own `xfrm` alone).
    pub start_connection: Option<ShapeConnection>,
    /// `<a:endCxn id=".." idx=".."/>` — same as `start_connection`, for the connector's end point.
    pub end_connection: Option<ShapeConnection>,
}

impl Connector {
    /// Creates a floating connector (no shape attachments) with no explicit position/size/line.
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            properties: ShapeProperties::new(),
            start_connection: None,
            end_connection: None,
        }
    }

    /// Sets this connector's position/size/line/fill.
    pub fn with_properties(mut self, properties: ShapeProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Attaches this connector's start point to another shape's connection site.
    pub fn with_start_connection(mut self, shape_id: u32, index: u32) -> Self {
        self.start_connection = Some(ShapeConnection { shape_id, index });
        self
    }

    /// Attaches this connector's end point to another shape's connection site.
    pub fn with_end_connection(mut self, shape_id: u32, index: u32) -> Self {
        self.end_connection = Some(ShapeConnection { shape_id, index });
        self
    }
}

/// One end of a [`Connector`]'s attachment to another shape (`<a:stCxn>`/`<a:endCxn>`,
/// `CT_Connection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeConnection {
    /// `id` — the attached shape's `<p:cNvPr id="..">`.
    pub shape_id: u32,
    /// `idx` — which of that shape's connection sites (a preset-shape- specific numbered list of
    /// attachment points around its outline; this crate doesn't validate the index against the
    /// target shape's actual preset).
    pub index: u32,
}

/// An embedded table (`<p:graphicFrame>` wrapping `<a:tbl>`, `a:graphicData
/// uri="../drawingml/2006/table"`) — DrawingML's own table vocabulary, distinct from
/// `word-ooxml::Table`'s WordprocessingML `w:tbl` (a different XML namespace/vocabulary entirely,
/// despite the conceptual overlap — no code is shared between the two, same posture as every other
/// cross-crate "same real-world concept, separate small copy" case in this workspace, e.g.
/// `PictureFormat`).
///
/// Structure (`p:graphicFrame`/`p:xfrm`/`a:tbl`/`a:tblGrid`/`a:tr`/`a:tc`) confirmed against real
/// fixtures: note the frame's own transform element is `<p:xfrm>`, not `<a:xfrm>`, same as
/// [`SlideChart`]'s. `<a:tableStyleId>` (a GUID referencing a table style) is written from
/// `style_id` when set (see [`SlideTableStyle`]), otherwise falls back to the same
/// fixed built-in default this crate's `tableStyles.xml` always declares, matching this crate's
/// existing posture on `p:xfrm`/theme boilerplate elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideTable {
    /// `<p:cNvPr id="..">` — same uniqueness rules as `AutoShape::id`.
    pub id: u32,
    /// `<p:cNvPr name="..">`.
    pub name: String,
    /// `<p:xfrm><a:off x=".." y="..">`, in EMUs.
    pub offset_emu: (i64, i64),
    /// `<p:xfrm><a:ext cx=".." cy="..">`, in EMUs.
    pub extent_emu: (i64, i64),
    /// `<a:tblGrid>`'s `<a:gridCol w="..">` widths, in EMUs, one per column (column count is
    /// derived from this, mirroring `word_ooxml::Table.column_widths`).
    pub column_widths_emu: Vec<i64>,
    pub rows: Vec<TableRow>,
    /// `<a:tableStyleId>` — references one of `Presentation.table_styles` by
    /// [`SlideTableStyle::id`]. `None` (the default) falls back to this crate's fixed built-in
    /// style, unchanged.
    pub style_id: Option<String>,
    /// `<a:tblPr firstRow=".">` — applies `style_id`'s `first_row` part to this table's actual
    /// first row (a header-row emphasis, e.g. bold text/a distinct fill). `false` (the default,
    /// matching `CT_TableProperties`'s own schema default) omits the attribute. **Without this, a
    /// referenced [`SlideTableStyle`]'s per-part formatting has no visible effect in real
    /// PowerPoint at all** — these six toggles are what actually turns each style part "on" for a
    /// given table; `style_id` alone only makes the style *available*. Grounded against a real
    /// fixture (`firstRow="1" bandRow="1"`).
    pub style_first_row: bool,
    /// `<a:tblPr firstCol="..">` — applies `style_id`'s `first_column` part. See
    /// [`SlideTable::style_first_row`]'s doc comment.
    pub style_first_column: bool,
    /// `<a:tblPr lastRow="..">` — applies `style_id`'s `last_row` part (a totals-row emphasis). See
    /// [`SlideTable::style_first_row`]'s doc comment.
    pub style_last_row: bool,
    /// `<a:tblPr lastCol="..">` — applies `style_id`'s `last_column` part. See
    /// [`SlideTable::style_first_row`]'s doc comment.
    pub style_last_column: bool,
    /// `<a:tblPr bandRow="..">` — applies `style_id`'s `band1_horizontal`/ `band2_horizontal` parts
    /// as alternating row stripes. See [`SlideTable::style_first_row`]'s doc comment.
    pub style_band_rows: bool,
    /// `<a:tblPr bandCol="..">` — applies `style_id`'s `band1_vertical`/ `band2_vertical` parts as
    /// alternating column stripes. See [`SlideTable::style_first_row`]'s doc comment.
    pub style_band_columns: bool,
}

impl SlideTable {
    /// Creates an empty table (no rows/columns) at the given size, positioned at `(0, 0)`, using
    /// the fixed built-in table style with none of its parts (first row/column, last row/column,
    /// banding) actually applied.
    pub fn new(id: u32, name: impl Into<String>, width_emu: i64, height_emu: i64) -> Self {
        Self {
            id,
            name: name.into(),
            offset_emu: (0, 0),
            extent_emu: (width_emu, height_emu),
            column_widths_emu: Vec::new(),
            rows: Vec::new(),
            style_id: None,
            style_first_row: false,
            style_first_column: false,
            style_last_row: false,
            style_last_column: false,
            style_band_rows: false,
            style_band_columns: false,
        }
    }

    /// Sets this table's position.
    pub fn with_offset(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.offset_emu = (x_emu, y_emu);
        self
    }

    /// Uses a custom table style (by id) instead of the fixed built-in one.
    pub fn with_style_id(mut self, style_id: impl Into<String>) -> Self {
        self.style_id = Some(style_id.into());
        self
    }

    /// Sets the column widths (and, implicitly, the column count).
    pub fn with_column_widths(mut self, column_widths_emu: impl Into<Vec<i64>>) -> Self {
        self.column_widths_emu = column_widths_emu.into();
        self
    }

    /// Appends a row.
    pub fn with_row(mut self, row: TableRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Applies the referenced style's first-row part to this table's actual first row (e.g. a bold
    /// header row).
    pub fn with_style_first_row(mut self, apply: bool) -> Self {
        self.style_first_row = apply;
        self
    }

    /// Applies the referenced style's first-column part.
    pub fn with_style_first_column(mut self, apply: bool) -> Self {
        self.style_first_column = apply;
        self
    }

    /// Applies the referenced style's last-row part (e.g. a totals row).
    pub fn with_style_last_row(mut self, apply: bool) -> Self {
        self.style_last_row = apply;
        self
    }

    /// Applies the referenced style's last-column part.
    pub fn with_style_last_column(mut self, apply: bool) -> Self {
        self.style_last_column = apply;
        self
    }

    /// Applies the referenced style's horizontal-banding parts (alternating row stripes).
    pub fn with_style_band_rows(mut self, apply: bool) -> Self {
        self.style_band_rows = apply;
        self
    }

    /// Applies the referenced style's vertical-banding parts (alternating column stripes).
    pub fn with_style_band_columns(mut self, apply: bool) -> Self {
        self.style_band_columns = apply;
        self
    }
}

/// One row of a [`SlideTable`] (`<a:tr h="..">`, `CT_TableRow`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableRow {
    /// `h` — row height, in EMUs.
    pub height_emu: i64,
    pub cells: Vec<TableCell>,
}

impl TableRow {
    /// Creates an empty row (no cells) at the given height.
    pub fn new(height_emu: i64) -> Self {
        Self {
            height_emu,
            cells: Vec::new(),
        }
    }

    /// Appends a cell.
    pub fn with_cell(mut self, cell: TableCell) -> Self {
        self.cells.push(cell);
        self
    }
}

/// One cell of a [`TableRow`] (`<a:tc>`, `CT_TableCell`).
///
/// **The merge fields are not grounded on a real fixture** — see this module's own top-level doc
/// comment; none of this project's real table fixtures happens to exercise a merged cell, only the
/// base `graphicFrame`/`tbl`/`tblGrid`/`tr`/`tc` structure itself is fixture- confirmed. Built from
/// ECMA-376's own `CT_TableCell` attribute list, which is genuinely different from
/// `word_ooxml::TableCell`'s merge model despite the conceptual overlap: WordprocessingML lets a
/// spanned cell be omitted from its row entirely (`gridSpan` absorbs the columns, no placeholder
/// `w:tc` needed for what it covers), but `CT_TableCell` has no such shortcut — every row must
/// carry exactly one `<a:tc>` per `<a:gridCol>`, and a merge is expressed by marking the *absorbed*
/// cells with `hMerge`/`vMerge` (plain booleans, not an enum) rather than omitting them, while the
/// top/left cell of the merge carries `gridSpan`/`rowSpan` (the count, not a boolean).
/// [`SlideTable`]'s writer enforces the "one cell per grid column, every row" shape by padding a
/// short row with empty cells rather than silently producing an invalid `.pptx`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCell {
    pub text_body: Option<TextBody>,
    /// `gridSpan` — how many grid columns this cell spans (a horizontal merge, the top/left cell
    /// only). `None` means `1` (no merge), matching `CT_TableCell`'s own schema default.
    pub horizontal_span: Option<u32>,
    /// `rowSpan` — how many rows this cell spans (a vertical merge, the top/left cell only). `None`
    /// means `1`.
    pub vertical_span: Option<u32>,
    /// `hMerge` — `true` marks this cell as absorbed by a horizontal merge from the cell to its
    /// left (this cell must still be present in the row, typically with no text of its own).
    pub horizontal_merge: bool,
    /// `vMerge` — `true` marks this cell as absorbed by a vertical merge from the cell above it.
    pub vertical_merge: bool,
    /// `<a:tcPr>` (`CT_TableCellProperties`) — this cell's own margins, vertical anchor, borders,
    /// and background fill, overriding whatever the table's style would otherwise apply. `None`
    /// (the default) omits `<a:tcPr>` entirely — the cell simply inherits everything from the table
    /// style, same "no override" default every other optional field in this crate uses.
    pub properties: Option<TableCellProperties>,
}

impl TableCell {
    /// Creates an empty cell (no text, no merge, no property overrides).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets this cell's text content.
    pub fn with_text_body(mut self, text_body: TextBody) -> Self {
        self.text_body = Some(text_body);
        self
    }

    /// Sets this cell's own margin/anchor/border/fill overrides (`<a:tcPr>`).
    pub fn with_properties(mut self, properties: TableCellProperties) -> Self {
        self.properties = Some(properties);
        self
    }

    /// Marks this cell as the top/left cell of a horizontal merge spanning `span` grid columns —
    /// the `span - 1` cells to its right still need to be present in the row, each created via
    /// [`TableCell::horizontally_merged`].
    pub fn with_horizontal_span(mut self, span: u32) -> Self {
        self.horizontal_span = Some(span);
        self
    }

    /// Marks this cell as the top/left cell of a vertical merge spanning `span` rows — the `span -
    /// 1` cells below it (same column, following rows) still need to be present, each created via
    /// [`TableCell::vertically_merged`].
    pub fn with_vertical_span(mut self, span: u32) -> Self {
        self.vertical_span = Some(span);
        self
    }

    /// An empty placeholder cell absorbed by a horizontal merge from its left neighbor
    /// (`hMerge="1"`).
    pub fn horizontally_merged() -> Self {
        Self {
            horizontal_merge: true,
            ..Self::default()
        }
    }

    /// An empty placeholder cell absorbed by a vertical merge from the cell above it
    /// (`vMerge="1"`).
    pub fn vertically_merged() -> Self {
        Self {
            vertical_merge: true,
            ..Self::default()
        }
    }
}

/// A [`TableCell`]'s own property overrides (`<a:tcPr>`, `CT_TableCellProperties`). Not modeled:
/// `anchorCtr`/`horzOverflow` attributes, the diagonal borders (`<a:lnTlToBr>`/`<a:lnBlToTr>`), and
/// `<a:cell3D>` (bevel/lighting effects) — all secondary refinements on top of margins, vertical
/// anchor, four straight borders, and background fill. Grounded against a real fixture, which
/// confirmed the attribute set and the
/// `lnL`/`lnR`/`lnT`/`lnB` then fill-choice element order (`CT_TableCellProperties`'s own
/// sequence).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCellProperties {
    /// `marL`, in EMUs.
    pub margin_left_emu: Option<i64>,
    /// `marT`, in EMUs.
    pub margin_top_emu: Option<i64>,
    /// `marR`, in EMUs.
    pub margin_right_emu: Option<i64>,
    /// `marB`, in EMUs.
    pub margin_bottom_emu: Option<i64>,
    /// `anchor` — vertical alignment of this cell's text within its own height. Reuses
    /// [`drawing::TextAnchor`] directly, the same type `<a:bodyPr anchor="..">` uses
    /// (`CT_TableCellProperties` and `CT_TextBodyProperties` both reference
    /// `ST_TextAnchoringType`).
    pub anchor: Option<TextAnchor>,
    /// `<a:lnL>` — this cell's left border.
    pub border_left: Option<Line>,
    /// `<a:lnR>` — this cell's right border.
    pub border_right: Option<Line>,
    /// `<a:lnT>` — this cell's top border.
    pub border_top: Option<Line>,
    /// `<a:lnB>` — this cell's bottom border.
    pub border_bottom: Option<Line>,
    /// `EG_FillProperties` — this cell's own background fill, overriding the table style's own cell
    /// fill.
    pub fill: Option<Fill>,
}

impl TableCellProperties {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets all four margins at once, in EMUs.
    pub fn with_margins_emu(mut self, left: i64, top: i64, right: i64, bottom: i64) -> Self {
        self.margin_left_emu = Some(left);
        self.margin_top_emu = Some(top);
        self.margin_right_emu = Some(right);
        self.margin_bottom_emu = Some(bottom);
        self
    }

    /// Sets this cell's vertical text anchor.
    pub fn with_anchor(mut self, anchor: TextAnchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Sets this cell's left border.
    pub fn with_border_left(mut self, border: Line) -> Self {
        self.border_left = Some(border);
        self
    }

    /// Sets this cell's right border.
    pub fn with_border_right(mut self, border: Line) -> Self {
        self.border_right = Some(border);
        self
    }

    /// Sets this cell's top border.
    pub fn with_border_top(mut self, border: Line) -> Self {
        self.border_top = Some(border);
        self
    }

    /// Sets this cell's bottom border.
    pub fn with_border_bottom(mut self, border: Line) -> Self {
        self.border_bottom = Some(border);
        self
    }

    /// Sets this cell's own background fill.
    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
        self
    }
}

/// A slide comment (`ppt/comments/commentN.xml`'s own `<p:cm>`, `CT_Comment`) — the legacy comment
/// mechanism ECMA-376 itself defines (pre-2016 "modern threaded comments", which PowerPoint now
/// stores in a different, Microsoft-proprietary way this crate doesn't model).
///
/// **Not grounded on a real fixture** — see this module's own top-level doc comment. Structure
/// follows ECMA-376's `CT_Comment`/`CT_CommentList`/ `CT_CommentAuthor`/`CT_CommentAuthorList`
/// directly (§19.4): a `<p:cm>`'s `authorId` attribute references a `<p:cmAuthor>` entry in the
/// package-wide `ppt/commentAuthors.xml` by numeric id — this crate hides that indirection
/// entirely, storing the author's name/initials directly on each `SlideComment` and having the
/// writer deduplicate/number authors automatically (an author appearing on several comments becomes
/// one `<p:cmAuthor>` entry, matching how real PowerPoint itself avoids duplicate author entries).
#[derive(Debug, Clone, PartialEq)]
pub struct SlideComment {
    /// `<p:cmAuthor name="..">` — this comment's author's display name.
    pub author: String,
    /// `<p:cmAuthor initials="..">`.
    pub initials: String,
    /// `<p:cm dt="..">` (`ST_DateTime`) — an ISO-8601 date/time string (e.g.
    /// `"2024-01-15T10:30:00.000"`). Stored as a plain string, same posture as every other
    /// date/time field in this workspace (no dedicated date/time type anywhere).
    pub date: String,
    /// `<p:pos x=".." y="..">`, in EMUs — where the comment's marker sits on the slide.
    pub position_emu: (i64, i64),
    /// `<p:text>` — the comment's own body text (plain text only, unlike e.g.
    /// `word_ooxml::Comment`'s block-level content — `CT_Comment`'s `text` is a plain `xsd:string`,
    /// no rich formatting at all).
    pub text: String,
}

impl SlideComment {
    /// Creates a comment at position `(0, 0)`.
    pub fn new(
        author: impl Into<String>,
        initials: impl Into<String>,
        date: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            author: author.into(),
            initials: initials.into(),
            date: date.into(),
            position_emu: (0, 0),
            text: text.into(),
        }
    }

    /// Sets this comment's marker position.
    pub fn with_position(mut self, x_emu: i64, y_emu: i64) -> Self {
        self.position_emu = (x_emu, y_emu);
        self
    }
}

/// A presentation's customizable color/font theme (`ppt/theme/theme1.xml`'s own
/// `<a:clrScheme>`/`<a:fontScheme>`).
///
/// Deliberately mirrors `word_ooxml::Theme`/`ColorScheme`/`FontScheme` by name and shape, not by
/// crate dependency — this workspace's established posture for a shared real-world concept with no
/// shared crate backing it (see `PictureFormat`'s own doc comment for the same reasoning). Unlike
/// `word_ooxml::Theme` (genuinely optional — `word/theme/theme1.xml` is only written when
/// `Document.theme` is set), a PowerPoint theme part is *mandatory*: `Presentation.theme: None`
/// still always produces a full `theme1.xml`, just with [`Theme::office_default`]'s colors/fonts
/// rather than a caller-supplied palette — see `Presentation`'s own doc comment.
///
/// Real PowerPoint files can and do carry more than one distinct theme (a real fixture has both
/// `theme1.xml` and `theme2.xml`, the latter the notes master's own); this crate, like
/// `word-ooxml`, still only exposes a single customizable theme for the slide master (the notes
/// master's own separate theme part stays fixed at `Theme::office_default`'s content regardless).
///
/// `fmtScheme` (`CT_BaseStyles` requires it alongside `clrScheme`/ `fontScheme`) stays a fixed,
/// non-configurable copy of Office's own built-in default, same scope limit as
/// `word_ooxml::Theme`'s own `fmtScheme` — irrelevant to `word-ooxml` there, but genuinely relevant
/// to a slide's shapes here; still out of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The theme's name (`<a:theme name="..">`), also reused unchanged as the nested
    /// `<a:clrScheme>`/`<a:fontScheme>` elements' own `name` attribute, same convention as
    /// `word_ooxml::Theme`.
    pub name: String,
    pub colors: ColorScheme,
    pub fonts: FontScheme,
}

/// A theme's 12-color palette (`<a:clrScheme>`, `CT_ColorScheme`) — mirrors
/// `word_ooxml::ColorScheme` by name/shape (see that type's own doc comment for the full rationale,
/// including why every slot is a plain RGB hex string rather than modeling `<a:sysClr>`'s live
/// system-color binding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    pub dark1: String,
    pub light1: String,
    pub dark2: String,
    pub light2: String,
    pub accent1: String,
    pub accent2: String,
    pub accent3: String,
    pub accent4: String,
    pub accent5: String,
    pub accent6: String,
    pub hyperlink: String,
    pub followed_hyperlink: String,
}

/// A theme's font scheme (`<a:fontScheme>`, `CT_FontScheme`) — mirrors `word_ooxml::FontScheme` by
/// name/shape (Latin typeface only, same scope limit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontScheme {
    pub major_latin: String,
    pub minor_latin: String,
}

impl Theme {
    /// Office's own built-in default theme ("Office": Calibri Light/ Calibri, the classic
    /// blue-accented palette) — byte-for-byte the same colors/fonts this crate's writer has always
    /// produced as its fixed theme, now expressed as data instead of a hardcoded XML literal.
    pub fn office_default() -> Self {
        Self {
            name: "Office".to_string(),
            colors: ColorScheme::office_default(),
            fonts: FontScheme::office_default(),
        }
    }
}

impl ColorScheme {
    pub fn office_default() -> Self {
        Self {
            dark1: "000000".to_string(),
            light1: "FFFFFF".to_string(),
            dark2: "44546A".to_string(),
            light2: "E7E6E6".to_string(),
            accent1: "4472C4".to_string(),
            accent2: "ED7D31".to_string(),
            accent3: "A5A5A5".to_string(),
            accent4: "FFC000".to_string(),
            accent5: "5B9BD5".to_string(),
            accent6: "70AD47".to_string(),
            hyperlink: "0563C1".to_string(),
            followed_hyperlink: "954F72".to_string(),
        }
    }
}

impl FontScheme {
    pub fn office_default() -> Self {
        Self {
            major_latin: "Calibri Light".to_string(),
            minor_latin: "Calibri".to_string(),
        }
    }
}

/// Document metadata (`docProps/core.xml`'s Dublin Core fields plus `docProps/app.xml`'s
/// `Company`/`Manager`/`HyperlinkBase`, and `docProps/custom.xml`'s user-defined properties). A
/// direct port of `excel-ooxml::DocumentProperties`'s own shape (same fields, same posture — see
/// that type's own doc comment for the full rationale); no PowerPoint-specific fields added, since
/// `docProps/app.xml`'s schema (`CT_Properties`, extended-properties) is shared verbatim across
/// every OOXML application, only differing in which app-specific statistics fields (`Slides` here
/// vs. `Sheets`/`Words` elsewhere) each host actually sets — this crate already sets `Slides`
/// unconditionally in `writer.rs`, independent of this type.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocumentProperties {
    /// `docProps/core.xml`'s `<dc:title>`.
    pub title: Option<String>,
    /// `docProps/core.xml`'s `<dc:creator>`.
    pub author: Option<String>,
    /// `docProps/core.xml`'s `<dc:subject>`.
    pub subject: Option<String>,
    /// `docProps/core.xml`'s `<cp:keywords>`.
    pub keywords: Option<String>,
    /// `docProps/app.xml`'s `<Company>`.
    pub company: Option<String>,
    /// `docProps/app.xml`'s `<Manager>`.
    pub manager: Option<String>,
    /// `docProps/app.xml`'s `<HyperlinkBase>`.
    pub hyperlink_base: Option<String>,
    /// User-defined custom document properties (`docProps/custom.xml`,
    /// `CT_Properties`/`CT_Property`). A `Vec` of `(name, value)` pairs, not a map: insertion order
    /// is preserved and determines the sequential `pid` each entry gets when written, matching
    /// `excel-ooxml`/`word-ooxml`'s own `custom_properties` convention exactly. Only written if
    /// non-empty.
    pub custom_properties: Vec<(String, CustomPropertyValue)>,
}

/// A single custom document property's value — same 4 variants as
/// `excel-ooxml::CustomPropertyValue`/`word_ooxml::CustomPropertyValue` (`CT_Property`'s value
/// `xsd:choice` has several more — vectors, blobs, currency, a date variant — not modeled here,
/// same "simple case first" posture as those two sibling types).
#[derive(Debug, Clone, PartialEq)]
pub enum CustomPropertyValue {
    /// A text value (`vt:lpwstr`).
    Text(String),
    /// A boolean value (`vt:bool`).
    Bool(bool),
    /// A 32-bit signed integer value (`vt:i4`).
    Int(i32),
    /// A 64-bit floating-point value (`vt:r8`).
    Number(f64),
}

impl DocumentProperties {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }

    pub fn with_company(mut self, company: impl Into<String>) -> Self {
        self.company = Some(company.into());
        self
    }

    pub fn with_manager(mut self, manager: impl Into<String>) -> Self {
        self.manager = Some(manager.into());
        self
    }

    pub fn with_hyperlink_base(mut self, hyperlink_base: impl Into<String>) -> Self {
        self.hyperlink_base = Some(hyperlink_base.into());
        self
    }

    /// Appends one custom document property.
    pub fn with_custom_property(
        mut self,
        name: impl Into<String>,
        value: CustomPropertyValue,
    ) -> Self {
        self.custom_properties.push((name.into(), value));
        self
    }
}

/// A custom table style (`<a:tblStyle styleId="." styleName=".">`, `CT_TableStyle`). Package-wide
/// ([`Presentation::table_styles`]), referenced by id from [`SlideTable::style_id`].
///
/// Grounded against a real fixture (`ppt/tableStyles.xml`, the built-in "Medium Style 2 - Accent 1"
/// style). Real `CT_TableStyle` has 12 optional "table part" children
/// (`wholeTbl`/`band1H`/`band2H`/`band1V`/`band2V`/`firstRow`/`lastRow`/
/// `firstCol`/`lastCol`/`neCell`/`nwCell`/`seCell`/`swCell`); this crate models the 9
/// commonly-authored ones (everything but the four single- corner-cell variants, a real-world
/// rarity). Within each part (`CT_TablePartStyle`), only fill + bold/italic + text color are
/// modeled (see [`TableStylePart`]) — no per-side border customization (`CT_TableCellBorderStyle`)
/// and no `fontRef` index, both schema- optional. A deliberate scope reduction, not a
/// fixture-exhaustive mirror of every real PowerPoint table style's structure.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlideTableStyle {
    /// `styleId` (`ST_Guid`) — referenced from [`SlideTable::style_id`].
    pub id: String,
    /// `styleName`.
    pub name: String,
    /// `<a:wholeTbl>` — the base style every other part layers on top of.
    pub whole_table: Option<TableStylePart>,
    /// `<a:band1H>` — odd row banding.
    pub band1_horizontal: Option<TableStylePart>,
    /// `<a:band2H>` — even row banding.
    pub band2_horizontal: Option<TableStylePart>,
    /// `<a:band1V>` — odd column banding.
    pub band1_vertical: Option<TableStylePart>,
    /// `<a:band2V>` — even column banding.
    pub band2_vertical: Option<TableStylePart>,
    /// `<a:firstRow>` — header row.
    pub first_row: Option<TableStylePart>,
    /// `<a:lastRow>` — total/footer row.
    pub last_row: Option<TableStylePart>,
    /// `<a:firstCol>`.
    pub first_column: Option<TableStylePart>,
    /// `<a:lastCol>`.
    pub last_column: Option<TableStylePart>,
}

impl SlideTableStyle {
    /// Creates a table style with the given id/name and no parts set yet (an empty `<a:tblStyle>` —
    /// schema-valid, though visually a no-op until at least one part is set).
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn with_whole_table(mut self, part: TableStylePart) -> Self {
        self.whole_table = Some(part);
        self
    }

    pub fn with_band1_horizontal(mut self, part: TableStylePart) -> Self {
        self.band1_horizontal = Some(part);
        self
    }

    pub fn with_band2_horizontal(mut self, part: TableStylePart) -> Self {
        self.band2_horizontal = Some(part);
        self
    }

    pub fn with_band1_vertical(mut self, part: TableStylePart) -> Self {
        self.band1_vertical = Some(part);
        self
    }

    pub fn with_band2_vertical(mut self, part: TableStylePart) -> Self {
        self.band2_vertical = Some(part);
        self
    }

    pub fn with_first_row(mut self, part: TableStylePart) -> Self {
        self.first_row = Some(part);
        self
    }

    pub fn with_last_row(mut self, part: TableStylePart) -> Self {
        self.last_row = Some(part);
        self
    }

    pub fn with_first_column(mut self, part: TableStylePart) -> Self {
        self.first_column = Some(part);
        self
    }

    pub fn with_last_column(mut self, part: TableStylePart) -> Self {
        self.last_column = Some(part);
        self
    }
}

/// One "table part" style (`CT_TableStyleTextStyle` + `CT_TableStyleCellStyle` combined — real
/// PowerPoint always writes both wrapped in the same `<a:wholeTbl>`/`<a:band1H>`/etc. element, so
/// this crate models them as a single value rather than two separate optional fields) — see
/// [`SlideTableStyle`]'s own doc comment for scope.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableStylePart {
    /// `<a:tcStyle><a:fill>{fill}</a:fill></a:tcStyle>` — the cell's own background. `None` omits
    /// `<a:fill>` (inherits).
    pub fill: Option<Fill>,
    /// `<a:tcTxStyle b="on">` — bold text.
    pub bold: bool,
    /// `<a:tcTxStyle i="on">` — italic text.
    pub italic: bool,
    /// `<a:tcTxStyle>{color}</a:tcTxStyle>` — the cell's own text color. `None` omits the color
    /// choice (inherits). Reuses [`Color`] verbatim — like the rest of this crate's own color
    /// usage, a theme-relative `<a:schemeClr>` reference isn't modeled.
    pub text_color: Option<Color>,
}

impl TableStylePart {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn with_text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }
}

/// A TrueType/OpenType font embedded directly in the package (`<p:embeddedFont><p:font
/// typeface="."/>{variants}</p:embeddedFont>`, `CT_EmbeddedFontListEntry`).
///
/// **Not grounded on a real fixture** — no `.pptx` in this project's local corpus embeds a font.
/// Built directly from ECMA-376's own `CT_EmbeddedFontListEntry`/ `CT_EmbeddedFontDataId` schema
/// text instead — flagged for extra scrutiny once opened in real PowerPoint, same posture already
/// used for [`Connector`]/[`SlideComment`] and [`crate::MediaFormat`]'s `p14:media` non-support.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmbeddedFont {
    /// `<p:font typeface="..">` — the font family name real PowerPoint resolves text runs against.
    pub typeface: String,
    /// `<p:regular r:id="..">`'s own `.fntdata` bytes.
    pub regular: Option<Vec<u8>>,
    /// `<p:bold r:id="..">`'s own `.fntdata` bytes.
    pub bold: Option<Vec<u8>>,
    /// `<p:italic r:id="..">`'s own `.fntdata` bytes.
    pub italic: Option<Vec<u8>>,
    /// `<p:boldItalic r:id="..">`'s own `.fntdata` bytes.
    pub bold_italic: Option<Vec<u8>>,
}

impl EmbeddedFont {
    /// Creates an embedded font entry with no variants set yet (use
    /// `with_regular`/`with_bold`/`with_italic`/`with_bold_italic` to attach at least one).
    pub fn new(typeface: impl Into<String>) -> Self {
        Self {
            typeface: typeface.into(),
            ..Default::default()
        }
    }

    pub fn with_regular(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.regular = Some(data.into());
        self
    }

    pub fn with_bold(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.bold = Some(data.into());
        self
    }

    pub fn with_italic(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.italic = Some(data.into());
        self
    }

    pub fn with_bold_italic(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.bold_italic = Some(data.into());
        self
    }
}
