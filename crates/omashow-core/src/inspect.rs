//! Read-only slide inspection: deck metadata, per-slide shapes, text runs, and
//! bounding boxes — the headless view behind the CLI `inspect` command and the
//! Tauri slide viewer.
//!
//! Everything here is derived from the in-memory [`Presentation`]; nothing in
//! this module mutates or re-serializes the deck.

use base64::Engine as _;
use serde::Serialize;

use office_toolkit::drawing::{Color, Fill, Line, TextAlign, TextBody, TextRun, TextRunProperties};
use office_toolkit::powerpoint::{
    MediaFormat, Picture, PictureFormat, PlaceholderKind, Presentation, Shape, ShapeGroup,
    SlideMedia, SlideTable, EMU_PER_INCH,
};

use crate::error::Error;
use crate::layout_geom::{xml_token, PhKey, PhMap};
use crate::model::text_body_to_string;

/// Slide canvas size in EMUs (`<p:sldSz cx=".." cy="..">`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SlideDimensions {
    pub width_emu: i64,
    pub height_emu: i64,
}

impl SlideDimensions {
    pub fn width_inches(&self) -> f64 {
        self.width_emu as f64 / EMU_PER_INCH as f64
    }

    pub fn height_inches(&self) -> f64 {
        self.height_emu as f64 / EMU_PER_INCH as f64
    }
}

/// Number of slides in the deck.
pub fn slide_count(pres: &Presentation) -> usize {
    pres.slides.len()
}

/// Slide canvas size in EMUs.
pub fn slide_dimensions(pres: &Presentation) -> SlideDimensions {
    SlideDimensions {
        width_emu: pres.slide_width_emu,
        height_emu: pres.slide_height_emu,
    }
}

/// The slide's custom background fill (`<p:bg>`) as a CSS value — a solid
/// color, or the token "none"/"gradient"/"pattern"/"image" for non-solid
/// fills. `None` when the slide inherits its background from its layout.
pub fn slide_background(pres: &Presentation, slide: usize) -> Result<Option<String>, Error> {
    let s = pres.slides.get(slide).ok_or(Error::OutOfRange(slide))?;
    Ok(s.background.as_ref().map(fill_to_css))
}

/// Axis-aligned bounding box in slide coordinates (top-left origin, x right,
/// y down), in EMUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoundingBox {
    pub x_emu: i64,
    pub y_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
}

/// One text run, flattened across the shape's paragraphs in document order.
/// Line breaks inside a paragraph appear as their own run with text `"\n"`.
#[derive(Debug, Clone, Serialize)]
pub struct TextRunInfo {
    /// 0-based index of the paragraph the run belongs to.
    pub paragraph: usize,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// `sz` in points (e.g. `18.0`); `None` when the run inherits its size
    /// and no placeholder-level default is known. For placeholders, the
    /// master's `<p:txStyles>` size is reported here when the run itself
    /// declares none.
    pub font_size_pt: Option<f64>,
    pub font_family: Option<String>,
    /// Text color as a CSS value (see [`color_to_css`]); `None` when the run
    /// inherits its color from the paragraph/shape defaults.
    pub color: Option<String>,
    /// Paragraph alignment as a CSS value (`"left"`, `"center"`, `"right"`,
    /// `"justify"`); `None` when the paragraph inherits the default (left).
    pub alignment: Option<String>,
}

/// A shape's outline stroke.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineInfo {
    /// Stroke width in EMUs; `None` when the shape uses the default width.
    pub width_emu: Option<i64>,
    /// Stroke color as a CSS value; `None` when the shape has no explicit line.
    pub color: Option<String>,
}

/// Embedded image data of a picture shape, resolved from the deck's media
/// parts (`ppt/media/*`) through the shape's `r:embed` relationship.
#[derive(Debug, Clone, Serialize)]
pub struct PicInfo {
    /// Image format: "png", "jpeg", "gif", or "bmp".
    pub format: &'static str,
    /// The raw image bytes as a `data:` URI, ready for an `<img src>`.
    pub data_uri: String,
    /// Size of the raw image in bytes.
    pub size_bytes: usize,
}

/// Embedded audio/video clip data of a media shape, resolved from the deck's
/// media parts (`ppt/media/*`) through the shape's media relationship.
#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    /// Clip kind: "video" or "audio".
    pub media_type: &'static str,
    /// The raw clip bytes as a `data:` URI, ready for an `<video src>` /
    /// `<audio src>`.
    pub data_uri: String,
    /// Size of the raw clip in bytes.
    pub size_bytes: usize,
}

/// One cell inside an OpenXML table.
#[derive(Debug, Clone, Serialize)]
pub struct TableCellInfo {
    /// Text paragraphs joined by `\n`; `None` if cell has no text.
    pub text: Option<String>,
    /// Text runs inside the cell.
    pub runs: Vec<TextRunInfo>,
    /// Background fill of the cell as CSS color or token.
    pub fill: Option<String>,
    /// Top border outline, if explicitly specified.
    pub border_top: Option<LineInfo>,
    /// Bottom border outline, if explicitly specified.
    pub border_bottom: Option<LineInfo>,
    /// Left border outline, if explicitly specified.
    pub border_left: Option<LineInfo>,
    /// Right border outline, if explicitly specified.
    pub border_right: Option<LineInfo>,
    /// Column span (default 1).
    pub col_span: u32,
    /// Row span (default 1).
    pub row_span: u32,
}

/// Extracted OpenXML table structure (`a:tbl`).
#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    /// Width of each column in EMUs.
    pub column_widths_emu: Vec<i64>,
    /// Height of each row in EMUs.
    pub row_heights_emu: Vec<i64>,
    /// 2D grid of rows and cells (`[row_idx][col_idx]`).
    pub rows: Vec<Vec<TableCellInfo>>,
    /// Whether the table style has first row formatting enabled.
    pub first_row: bool,
    /// Whether the table style has first column formatting enabled.
    pub first_col: bool,
    /// Whether the table style has banded rows enabled.
    pub band_rows: bool,
    /// Whether the table style has banded columns enabled.
    pub band_cols: bool,
}

/// A connector's end anchor: the connected shape and its connection-site index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionInfo {
    pub shape_id: u32,
    pub index: u32,
}

/// Connection-site info for a connector shape (`stCxn` / `endCxn`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorInfo {
    pub start_connection: Option<ConnectionInfo>,
    pub end_connection: Option<ConnectionInfo>,
}

/// Serializable view of one shape. For a group, `children` holds the nested
/// shapes with their bounds already remapped out of the group's child
/// coordinate space into slide coordinates.
#[derive(Debug, Clone, Serialize)]
pub struct ShapeInfo {
    pub id: u32,
    pub name: String,
    /// One of `"autoshape"`, `"picture"`, `"chart"`, `"group"`, `"connector"`,
    /// `"table"`, `"media"`.
    pub kind: &'static str,
    /// Placeholder role (`"title"`, `"body"`, ...), when the shape is one.
    pub placeholder: Option<&'static str>,
    /// Bounds in slide coordinates. Placeholder autoshapes that omit their
    /// own transform inherit the matching layout/master placeholder's box
    /// (resolved via [`crate::layout_geom`]), so `None` only remains for
    /// shapes with no resolvable geometry at all.
    pub bounds: Option<BoundingBox>,
    /// The shape's full text, paragraphs joined by `\n`; `None` when the
    /// shape carries no text body.
    pub text: Option<String>,
    /// Interior fill as a CSS color (solid fills) or a token ("none",
    /// "gradient", "pattern", "image"); `None` when the shape inherits its
    /// fill from the theme/layout.
    pub fill: Option<String>,
    /// Outline stroke, when the shape declares one.
    pub line: Option<LineInfo>,
    /// Text runs in document order.
    pub runs: Vec<TextRunInfo>,
    /// Embedded image data, only for a `picture` shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pic: Option<PicInfo>,
    /// Embedded audio/video clip, only for a `media` shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaInfo>,
    /// Extracted table content and grid structure, only for a `table` shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TableInfo>,
    /// Connection-site info, only for a `connector` shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<ConnectorInfo>,
    /// Nested shapes, only for a group.
    pub children: Option<Vec<ShapeInfo>>,
}

/// Serializable view of every shape on slide `slide`, in document order.
///
/// Group children stay nested under their group, but with bounds remapped
/// into slide coordinates, so a flat consumer never has to re-derive the
/// group transform.
pub fn get_slide_shapes(pres: &Presentation, slide: usize) -> Result<Vec<ShapeInfo>, Error> {
    get_slide_shapes_geom(pres, slide, None)
}

/// [`get_slide_shapes`] with the slide's resolved placeholder-geometry table
/// (`layout_geom::PhMap`), so placeholder shapes that inherit their box from
/// the layout/master carry real bounds instead of `None`.
pub fn get_slide_shapes_geom(
    pres: &Presentation,
    slide: usize,
    geom: Option<&PhMap>,
) -> Result<Vec<ShapeInfo>, Error> {
    let slide = pres.slides.get(slide).ok_or(Error::OutOfRange(slide))?;
    Ok(slide
        .shapes
        .iter()
        .map(|s| shape_info(s, &GroupContext::identity(), geom))
        .collect())
}

/// How a group's child coordinate space maps into slide coordinates, per
/// axis: `slide = (child - c) * num / den + o`. Kept as a rational so nested
/// groups compose without intermediate rounding.
#[derive(Clone, Copy)]
struct GroupContext {
    cx: i128,
    cy: i128,
    ox: i128,
    oy: i128,
    num_x: i128,
    den_x: i128,
    num_y: i128,
    den_y: i128,
}

impl GroupContext {
    fn identity() -> Self {
        Self {
            cx: 0,
            cy: 0,
            ox: 0,
            oy: 0,
            num_x: 1,
            den_x: 1,
            num_y: 1,
            den_y: 1,
        }
    }

    fn map_axis(v: i128, c: i128, o: i128, num: i128, den: i128) -> i128 {
        if den == 0 {
            return v + o;
        }
        div_round((v - c) * num, den) + o
    }

    fn map_box(&self, x: i64, y: i64, w: i64, h: i64) -> BoundingBox {
        BoundingBox {
            x_emu: Self::map_axis(x as i128, self.cx, self.ox, self.num_x, self.den_x) as i64,
            y_emu: Self::map_axis(y as i128, self.cy, self.oy, self.num_y, self.den_y) as i64,
            width_emu: Self::map_axis(w as i128, 0, 0, self.num_x, self.den_x) as i64,
            height_emu: Self::map_axis(h as i128, 0, 0, self.num_y, self.den_y) as i64,
        }
    }

    /// Compose this context with a group's own child-space transform.
    fn nested(&self, g: &ShapeGroup) -> Self {
        let (ox_g, oy_g) = (g.offset_emu.0 as i128, g.offset_emu.1 as i128);
        let (ex_g, ey_g) = (g.extent_emu.0 as i128, g.extent_emu.1 as i128);
        let (cx_g, cy_g) = (g.child_offset_emu.0 as i128, g.child_offset_emu.1 as i128);
        let (cex_g, cey_g) = (g.child_extent_emu.0 as i128, g.child_extent_emu.1 as i128);

        // Composed scale: (child - cx_g) * ex_g / cex_g, then this context.
        // A zero child extent is degenerate — skip the rescale on that axis.
        let (num_x, den_x) = if cex_g == 0 {
            (self.num_x, self.den_x)
        } else {
            (self.num_x * ex_g, self.den_x * cex_g)
        };
        let (num_y, den_y) = if cey_g == 0 {
            (self.num_y, self.den_y)
        } else {
            (self.num_y * ey_g, self.den_y * cey_g)
        };

        Self {
            cx: cx_g,
            cy: cy_g,
            ox: Self::map_axis(ox_g, self.cx, self.ox, self.num_x, self.den_x),
            oy: Self::map_axis(oy_g, self.cy, self.oy, self.num_y, self.den_y),
            num_x,
            den_x,
            num_y,
            den_y,
        }
    }
}

/// Nearest-integer division (ties away from zero); caller guarantees `d != 0`.
fn div_round(n: i128, d: i128) -> i128 {
    let (q, r) = (n / d, n % d);
    let twice = r * 2;
    if twice >= d {
        q + 1
    } else if -twice >= d {
        q - 1
    } else {
        q
    }
}

fn shape_info(shape: &Shape, ctx: &GroupContext, geom: Option<&PhMap>) -> ShapeInfo {
    match shape {
        Shape::AutoShape(a) => {
            let placeholder = a.placeholder.as_ref().map(|p| placeholder_token(&p.kind));
            // Placeholder shapes without their own transform inherit position,
            // size, and default text size from the layout/master table.
            let inherited = a.placeholder.as_ref().and_then(|p| {
                let key: PhKey = (xml_token(&p.kind), p.index.unwrap_or(0));
                geom.and_then(|g| g.get(&key))
            });
            let bounds = a
                .properties
                .transform
                .as_ref()
                .and_then(|t| match (t.offset, t.extent) {
                    (Some((x, y)), Some((w, h))) => Some(ctx.map_box(x, y, w, h)),
                    _ => None,
                })
                .or_else(|| {
                    inherited.map(|g| {
                        ctx.map_box(
                            g.bounds.x_emu,
                            g.bounds.y_emu,
                            g.bounds.width_emu,
                            g.bounds.height_emu,
                        )
                    })
                });
            let default_size = inherited.and_then(|g| g.font_size_100ths_pt);
            ShapeInfo {
                id: a.id,
                name: a.name.clone(),
                kind: "autoshape",
                placeholder,
                bounds,
                text: a.text_body.as_ref().map(text_body_to_string),
                fill: a.properties.fill.as_ref().map(fill_to_css),
                line: a.properties.line.as_ref().map(line_info),
                runs: a
                    .text_body
                    .as_ref()
                    .map(|tb| flatten_runs(tb, default_size))
                    .unwrap_or_default(),
                pic: None,
                media: None,
                table: None,
                connector: None,
                children: None,
            }
        }
        Shape::Picture(p) => ShapeInfo {
            id: p.id,
            name: p.name.clone(),
            kind: "picture",
            placeholder: None,
            bounds: Some(ctx.map_box(
                p.offset_emu.0,
                p.offset_emu.1,
                p.extent_emu.0,
                p.extent_emu.1,
            )),
            text: None,
            fill: p
                .shape_properties
                .as_ref()
                .and_then(|sp| sp.fill.as_ref())
                .map(fill_to_css),
            line: p
                .shape_properties
                .as_ref()
                .and_then(|sp| sp.line.as_ref())
                .map(line_info),
            runs: Vec::new(),
            pic: Some(pic_info(p)),
            media: None,
            table: None,
            connector: None,
            children: None,
        },
        Shape::Chart(c) => ShapeInfo {
            id: c.id,
            name: c.name.clone(),
            kind: "chart",
            placeholder: None,
            bounds: Some(ctx.map_box(
                c.offset_emu.0,
                c.offset_emu.1,
                c.extent_emu.0,
                c.extent_emu.1,
            )),
            text: None,
            fill: None,
            line: None,
            runs: Vec::new(),
            pic: None,
            media: None,
            table: None,
            connector: None,
            children: None,
        },
        Shape::Group(g) => {
            // Placeholders only inherit from the layout/master at the
            // top level of a slide, so nested shapes resolve without the table.
            let children = g
                .shapes
                .iter()
                .map(|s| shape_info(s, &ctx.nested(g), None))
                .collect();
            ShapeInfo {
                id: g.id,
                name: g.name.clone(),
                kind: "group",
                placeholder: None,
                bounds: Some(ctx.map_box(
                    g.offset_emu.0,
                    g.offset_emu.1,
                    g.extent_emu.0,
                    g.extent_emu.1,
                )),
                text: None,
                fill: None,
                line: None,
                runs: Vec::new(),
                pic: None,
                media: None,
                table: None,
                connector: None,
                children: Some(children),
            }
        }
        Shape::Connector(c) => ShapeInfo {
            id: c.id,
            name: c.name.clone(),
            kind: "connector",
            placeholder: None,
            bounds: c
                .properties
                .transform
                .as_ref()
                .and_then(|t| match (t.offset, t.extent) {
                    (Some((x, y)), Some((w, h))) => Some(ctx.map_box(x, y, w, h)),
                    _ => None,
                }),
            text: None,
            fill: c.properties.fill.as_ref().map(fill_to_css),
            line: c.properties.line.as_ref().map(line_info),
            runs: Vec::new(),
            pic: None,
            media: None,
            table: None,
            connector: Some(ConnectorInfo {
                start_connection: c.start_connection.map(|sc| ConnectionInfo {
                    shape_id: sc.shape_id,
                    index: sc.index,
                }),
                end_connection: c.end_connection.map(|ec| ConnectionInfo {
                    shape_id: ec.shape_id,
                    index: ec.index,
                }),
            }),
            children: None,
        },
        Shape::Table(t) => ShapeInfo {
            id: t.id,
            name: t.name.clone(),
            kind: "table",
            placeholder: None,
            bounds: Some(ctx.map_box(
                t.offset_emu.0,
                t.offset_emu.1,
                t.extent_emu.0,
                t.extent_emu.1,
            )),
            text: None,
            fill: None,
            line: None,
            runs: Vec::new(),
            pic: None,
            media: None,
            table: Some(table_info(t)),
            connector: None,
            children: None,
        },
        Shape::Media(m) => ShapeInfo {
            id: m.id,
            name: m.name.clone(),
            kind: "media",
            placeholder: None,
            bounds: Some(ctx.map_box(
                m.offset_emu.0,
                m.offset_emu.1,
                m.extent_emu.0,
                m.extent_emu.1,
            )),
            text: None,
            fill: None,
            line: None,
            runs: Vec::new(),
            pic: None,
            media: Some(media_info(m)),
            table: None,
            connector: None,
            children: None,
        },
    }
}

fn table_info(t: &SlideTable) -> TableInfo {
    let row_heights_emu = t.rows.iter().map(|r| r.height_emu).collect();
    let rows = t
        .rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .map(|cell| {
                    let text = cell.text_body.as_ref().map(text_body_to_string);
                    let runs = cell
                        .text_body
                        .as_ref()
                        .map(|tb| flatten_runs(tb, None))
                        .unwrap_or_default();
                    let (fill, border_top, border_bottom, border_left, border_right) =
                        if let Some(props) = &cell.properties {
                            (
                                props.fill.as_ref().map(fill_to_css),
                                props.border_top.as_ref().map(line_info),
                                props.border_bottom.as_ref().map(line_info),
                                props.border_left.as_ref().map(line_info),
                                props.border_right.as_ref().map(line_info),
                            )
                        } else {
                            (None, None, None, None, None)
                        };
                    TableCellInfo {
                        text,
                        runs,
                        fill,
                        border_top,
                        border_bottom,
                        border_left,
                        border_right,
                        col_span: cell.horizontal_span.unwrap_or(1),
                        row_span: cell.vertical_span.unwrap_or(1),
                    }
                })
                .collect()
        })
        .collect();

    TableInfo {
        column_widths_emu: t.column_widths_emu.clone(),
        row_heights_emu,
        rows,
        first_row: t.style_first_row,
        first_col: t.style_first_column,
        band_rows: t.style_band_rows,
        band_cols: t.style_band_columns,
    }
}

fn pic_info(p: &Picture) -> PicInfo {
    let format = match p.format {
        PictureFormat::Png => "png",
        PictureFormat::Jpeg => "jpeg",
        PictureFormat::Gif => "gif",
        PictureFormat::Bmp => "bmp",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&p.data);
    PicInfo {
        format,
        data_uri: format!("data:image/{format};base64,{b64}"),
        size_bytes: p.data.len(),
    }
}

fn media_info(m: &SlideMedia) -> MediaInfo {
    // `MediaFormat::is_video`/`content_type` are crate-private in the toolkit,
    // so classify and name the MIME type here from the public variants.
    let (media_type, mime) = match m.format {
        MediaFormat::Mp4 => ("video", "video/mp4"),
        MediaFormat::Wmv => ("video", "video/x-ms-wmv"),
        MediaFormat::Avi => ("video", "video/avi"),
        MediaFormat::Mp3 => ("audio", "audio/mpeg"),
        MediaFormat::Wav => ("audio", "audio/wav"),
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&m.data);
    MediaInfo {
        media_type,
        data_uri: format!("data:{mime};base64,{b64}"),
        size_bytes: m.data.len(),
    }
}

fn placeholder_token(kind: &PlaceholderKind) -> &'static str {
    match kind {
        PlaceholderKind::Title => "title",
        PlaceholderKind::Body => "body",
        PlaceholderKind::CenterTitle => "ctrTitle",
        PlaceholderKind::SubTitle => "subTitle",
        PlaceholderKind::DateTime => "dateTime",
        PlaceholderKind::SlideNumber => "slideNumber",
        PlaceholderKind::Footer => "footer",
        PlaceholderKind::Header => "header",
        PlaceholderKind::Object => "object",
        PlaceholderKind::SlideImage => "sldImg",
        PlaceholderKind::Other(_) => "other",
    }
}

fn flatten_runs(tb: &TextBody, default_size_100ths_pt: Option<i32>) -> Vec<TextRunInfo> {
    let default_props = TextRunProperties::new();
    let mut runs = Vec::new();
    for (para_idx, para) in tb.paragraphs.iter().enumerate() {
        let alignment = para
            .properties
            .as_ref()
            .and_then(|p| p.alignment)
            .map(alignment_to_css);
        for run in &para.runs {
            let (text, properties) = match run {
                TextRun::Regular { text, properties } => (text.as_str(), properties),
                TextRun::LineBreak { properties } => {
                    ("\n", properties.as_ref().unwrap_or(&default_props))
                }
                TextRun::Field {
                    cached_text,
                    properties,
                    ..
                } => (cached_text.as_str(), properties),
            };
            runs.push(run_info(
                para_idx,
                alignment.clone(),
                text,
                properties,
                default_size_100ths_pt,
            ));
        }
    }
    runs
}

fn run_info(
    paragraph: usize,
    alignment: Option<String>,
    text: &str,
    p: &TextRunProperties,
    default_size_100ths_pt: Option<i32>,
) -> TextRunInfo {
    TextRunInfo {
        paragraph,
        text: text.to_string(),
        bold: p.bold,
        italic: p.italic,
        font_size_pt: p
            .font_size_100ths_point
            .or(default_size_100ths_pt)
            .map(|sz| sz as f64 / 100.0),
        font_family: p.font_family.clone(),
        color: p.fill.as_ref().map(fill_to_css),
        alignment,
    }
}

/// Render a paragraph alignment as the matching CSS `text-align` value.
fn alignment_to_css(alignment: TextAlign) -> String {
    match alignment {
        TextAlign::Left => "left".to_string(),
        TextAlign::Center => "center".to_string(),
        TextAlign::Right => "right".to_string(),
        TextAlign::Justified
        | TextAlign::JustifiedLow
        | TextAlign::Distributed
        | TextAlign::ThaiDistributed => "justify".to_string(),
    }
}

/// Render an OpenXML color as a CSS value the frontend can use directly.
fn color_to_css(color: &Color) -> String {
    match color {
        Color::Rgb(hex) => format!("#{hex}"),
        Color::RgbPercent { red, green, blue } => {
            let to_byte = |v: i64| (v.clamp(0, 100_000) * 255 + 50_000) / 100_000;
            format!(
                "#{:02x}{:02x}{:02x}",
                to_byte(*red),
                to_byte(*green),
                to_byte(*blue)
            )
        }
        Color::Hsl {
            hue_60000ths,
            saturation_1000ths_percent,
            luminance_1000ths_percent,
        } => {
            format!(
                "hsl({}, {}%, {}%)",
                hue_60000ths / 60_000,
                saturation_1000ths_percent / 1_000,
                luminance_1000ths_percent / 1_000
            )
        }
        Color::System { value, last_color } => last_color
            .as_deref()
            .map(|c| format!("#{c}"))
            .unwrap_or_else(|| value.clone()),
        Color::Preset(name) => name.clone(),
    }
}

/// Render a fill as a CSS color for solid fills; non-solid fills degrade to a
/// short token the renderer can branch on ("none", "gradient", "pattern",
/// "image").
fn fill_to_css(fill: &Fill) -> String {
    match fill {
        Fill::None => "none".to_string(),
        Fill::Solid(color) => color_to_css(color),
        Fill::Gradient(_) => "gradient".to_string(),
        Fill::Pattern(_) => "pattern".to_string(),
        Fill::Image(_) => "image".to_string(),
    }
}

fn line_info(line: &Line) -> LineInfo {
    LineInfo {
        width_emu: line.width_emu,
        color: line.fill.as_ref().map(fill_to_css),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_to_css_covers_all_variants() {
        assert_eq!(color_to_css(&Color::Rgb("1A2B3C".to_string())), "#1A2B3C");
        // 100000 = 100% -> 255 -> ff; 0 -> 00.
        assert_eq!(
            color_to_css(&Color::RgbPercent {
                red: 100_000,
                green: 0,
                blue: 0
            }),
            "#ff0000"
        );
        assert_eq!(
            color_to_css(&Color::Hsl {
                hue_60000ths: 120_000, // 2 degrees
                saturation_1000ths_percent: 50_000,
                luminance_1000ths_percent: 25_000,
            }),
            "hsl(2, 50%, 25%)"
        );
        // System color prefers the authoring-time RGB fallback.
        assert_eq!(
            color_to_css(&Color::System {
                value: "windowText".to_string(),
                last_color: Some("000000".to_string()),
            }),
            "#000000"
        );
        assert_eq!(
            color_to_css(&Color::System {
                value: "btnFace".to_string(),
                last_color: None,
            }),
            "btnFace"
        );
        // Preset names pass through (already CSS-like).
        assert_eq!(color_to_css(&Color::Preset("tomato".to_string())), "tomato");
    }

    #[test]
    fn alignment_to_css_maps_all_variants() {
        assert_eq!(alignment_to_css(TextAlign::Left), "left");
        assert_eq!(alignment_to_css(TextAlign::Center), "center");
        assert_eq!(alignment_to_css(TextAlign::Right), "right");
        assert_eq!(alignment_to_css(TextAlign::Justified), "justify");
        assert_eq!(alignment_to_css(TextAlign::JustifiedLow), "justify");
        assert_eq!(alignment_to_css(TextAlign::Distributed), "justify");
        assert_eq!(alignment_to_css(TextAlign::ThaiDistributed), "justify");
    }

    #[test]
    fn fill_to_css_degrades_non_solid_fills_to_tokens() {
        assert_eq!(fill_to_css(&Fill::None), "none");
        assert_eq!(
            fill_to_css(&Fill::Solid(Color::Rgb("00FF00".to_string()))),
            "#00FF00"
        );
        assert_eq!(fill_to_css(&Fill::Gradient(Default::default())), "gradient");
    }
}
