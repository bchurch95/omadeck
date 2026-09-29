//! Vector PDF export: one PDF page per slide at 72 pt per inch, with shapes,
//! text and pictures placed at their exact EMU positions.
//!
//! Output is fully vector: shape bodies and borders are path objects, text
//! uses the PDF base-14 font set (family-mapped from the deck's fonts), and
//! pictures are embedded as image XObjects sized to their slide bounding box.

use std::path::Path;

use base64::Engine;
use printpdf::image::{Image, ImageTransform};
use printpdf::line::Polygon;
use printpdf::path::{PaintMode, WindingOrder};
use printpdf::point::Point;
use printpdf::scale::Mm;
use printpdf::{
    BuiltinFont, Color, PdfDocument, PdfDocumentReference, PdfLayerReference, Pt, Rgb, TextMatrix,
};

use crate::document::PptxDocument;
use crate::error::Error;
use crate::inspect::{BoundingBox, LineInfo, PicInfo, ShapeInfo, TextRunInfo};

/// 72 pt per inch, 914 400 EMU per inch.
const PT_PER_EMU: f64 = 72.0 / 914_400.0;
const MM_PER_PT: f32 = (25.4 / 72.0) as f32;
const DEFAULT_FONT_PT: f32 = 18.0;
const LINE_SPACING: f32 = 1.2;

impl PptxDocument {
    /// Render the whole deck to a vector PDF file: one page per slide,
    /// 1:1 EMU-to-point geometry.
    pub fn export_pdf(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let bytes = self.export_pdf_bytes()?;
        std::fs::write(path.as_ref(), bytes)?;
        Ok(())
    }

    /// Render the whole deck to a vector PDF and return the raw bytes.
    pub fn export_pdf_bytes(&self) -> Result<Vec<u8>, Error> {
        let dims = self.slide_dimensions();
        let w_mm = emu_to_mm(dims.width_emu);
        let h_mm = emu_to_mm(dims.height_emu);
        let (doc, first_page, first_layer) =
            PdfDocument::new("OmaDeck presentation", w_mm, h_mm, "slide 1");

        {
            let page = doc.get_page(first_page).get_layer(first_layer);
            paint_shapes(&doc, &page, &self.get_slide_shapes(0)?, h_mm)?;
        }
        for i in 1..self.slide_count() {
            let (page, layer) = doc.add_page(w_mm, h_mm, format!("slide {}", i + 1));
            let page = doc.get_page(page).get_layer(layer);
            paint_shapes(&doc, &page, &self.get_slide_shapes(i)?, h_mm)?;
        }
        doc.save_to_bytes().map_err(Error::from)
    }
}

fn emu_to_mm(emu: i64) -> Mm {
    Mm((emu as f64 * PT_PER_EMU * 25.4 / 72.0) as f32)
}

/// Slide-EMU rectangle to PDF coordinates: `(x_left, y_bottom, w, h)` in mm,
/// with the y-axis flipped (PDF origin is bottom-left, slide EMU top-left).
fn rect_in_mm(b: &BoundingBox, slide_h_mm: Mm) -> (Mm, Mm, Mm, Mm) {
    let x = emu_to_mm(b.x_emu);
    let y_top = emu_to_mm(b.y_emu);
    let w = emu_to_mm(b.width_emu);
    let h = emu_to_mm(b.height_emu);
    let bottom = slide_h_mm.0 - y_top.0 - h.0;
    (x, Mm(bottom), w, h)
}

fn rect_polygon(b: &BoundingBox, slide_h_mm: Mm, mode: PaintMode) -> Polygon {
    let (x, y, w, h) = rect_in_mm(b, slide_h_mm);
    Polygon {
        rings: vec![vec![
            (Point::new(x, y), false),
            (Point::new(Mm(x.0 + w.0), y), false),
            (Point::new(Mm(x.0 + w.0), Mm(y.0 + h.0)), false),
            (Point::new(x, Mm(y.0 + h.0)), false),
        ]],
        mode,
        winding_order: WindingOrder::NonZero,
    }
}

fn paint_shapes(
    doc: &PdfDocumentReference,
    layer: &PdfLayerReference,
    shapes: &[ShapeInfo],
    slide_h_mm: Mm,
) -> Result<(), Error> {
    for sh in shapes {
        if let Some(children) = &sh.children {
            paint_shapes(doc, layer, children, slide_h_mm)?;
            continue;
        }
        match sh.kind {
            "picture" => {
                if let (Some(b), Some(pic)) = (&sh.bounds, &sh.pic) {
                    paint_picture(layer, b, pic, slide_h_mm)?;
                }
            }
            "autoshape" => {
                if let Some(b) = &sh.bounds {
                    paint_autoshape(doc, layer, sh, b, slide_h_mm)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn paint_autoshape(
    doc: &PdfDocumentReference,
    layer: &PdfLayerReference,
    sh: &ShapeInfo,
    b: &BoundingBox,
    slide_h_mm: Mm,
) -> Result<(), Error> {
    if let Some(color) = sh.fill.as_deref().and_then(css_to_pdf_color) {
        layer.set_fill_color(color);
        layer.add_polygon(rect_polygon(b, slide_h_mm, PaintMode::Fill));
    }
    if let Some((color, width_pt)) = stroke_spec(sh.line.as_ref()) {
        layer.set_outline_color(color);
        layer.set_outline_thickness(width_pt);
        layer.add_polygon(rect_polygon(b, slide_h_mm, PaintMode::Stroke));
    }
    if !sh.runs.is_empty() {
        paint_text(doc, layer, sh, b, slide_h_mm)?;
    }
    Ok(())
}

fn stroke_spec(line: Option<&LineInfo>) -> Option<(Color, f32)> {
    let line = line?;
    let color = line.color.as_deref().and_then(css_to_pdf_color)?;
    let width_pt = (line.width_emu.unwrap_or(12_700) as f64 * PT_PER_EMU).max(0.25) as f32;
    Some((color, width_pt))
}

/// One styled run inside a text line: (text, bold, italic, size_pt, color, family).
type TextSeg = (String, bool, bool, f32, Option<String>, Option<String>);

struct TextLine {
    segs: Vec<TextSeg>,
    size_pt: f32,
    align: String,
}

fn new_line(align: Option<&str>) -> TextLine {
    TextLine {
        segs: Vec::new(),
        size_pt: 0.0,
        align: align.map(str::to_string).unwrap_or_default(),
    }
}

fn build_lines(runs: &[TextRunInfo]) -> Vec<TextLine> {
    let mut lines: Vec<TextLine> = Vec::new();
    let mut cur = new_line(None);
    let mut para = 0usize;
    for r in runs {
        if r.paragraph != para {
            if !cur.segs.is_empty() {
                lines.push(cur);
            }
            para = r.paragraph;
            cur = new_line(r.alignment.as_deref());
        }
        if cur.align.is_empty() {
            cur.align = r.alignment.clone().unwrap_or_default();
        }
        if r.text == "\n" {
            if !cur.segs.is_empty() {
                lines.push(cur);
            }
            cur = new_line(r.alignment.as_deref());
            continue;
        }
        if r.text.is_empty() {
            continue;
        }
        let size = r.font_size_pt.map(|s| s as f32).unwrap_or(DEFAULT_FONT_PT);
        cur.segs.push((
            r.text.clone(),
            r.bold,
            r.italic,
            size,
            r.color.clone(),
            r.font_family.clone(),
        ));
        cur.size_pt = cur.size_pt.max(size);
    }
    if !cur.segs.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Average advance of a glyph in a base-14 font, in pt. Used to position
/// centered and right-aligned lines without a full font metrics engine.
fn avg_advance(ch: char, size_pt: f32) -> f32 {
    let em = match ch {
        'i' | 'l' | 'j' | 't' | 'f' | '!' | '.' | ',' | ':' | ';' | '[' | ']' | '(' | ')' => 0.32,
        'm' | 'w' => 0.85,
        '\u{2013}' | '\u{2014}' => 0.9,
        ' ' => 0.28,
        c if c.is_uppercase() => 0.68,
        _ => 0.52,
    };
    em * size_pt
}

fn line_width_pt(line: &TextLine) -> f32 {
    line.segs
        .iter()
        .map(|(t, _, _, s, _, _)| t.chars().map(|c| avg_advance(c, *s)).sum::<f32>())
        .sum()
}

fn paint_text(
    doc: &PdfDocumentReference,
    layer: &PdfLayerReference,
    sh: &ShapeInfo,
    b: &BoundingBox,
    slide_h_mm: Mm,
) -> Result<(), Error> {
    let lines = build_lines(&sh.runs);
    if lines.is_empty() {
        return Ok(());
    }
    let (x, y_bottom, w, h) = rect_in_mm(b, slide_h_mm);
    let top_mm = y_bottom.0 + h.0;
    let mut offset_mm = 0.0f32;

    layer.begin_text_section();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            offset_mm += lines[i - 1].size_pt * LINE_SPACING * MM_PER_PT;
        }
        // PDF y grows upward: the first baseline sits ~0.9 em below the box
        // top, each following line one line-height further down.
        let baseline_mm = top_mm - offset_mm - line.size_pt * 0.9 * MM_PER_PT;
        let lw_mm = line_width_pt(line) * MM_PER_PT;
        let mut x_mm = match line.align.as_str() {
            "center" => x.0 + (w.0 - lw_mm) / 2.0,
            "right" => x.0 + w.0 - lw_mm,
            _ => x.0,
        };
        x_mm = x_mm.clamp(x.0, x.0 + w.0);
        for (text, bold, italic, seg_size, color, family) in &line.segs {
            let font = doc.add_builtin_font(font_for(family.as_deref(), *bold, *italic))?;
            let col = color
                .as_deref()
                .and_then(css_to_pdf_color)
                .unwrap_or_else(|| Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
            layer.set_font(&font, *seg_size);
            layer.set_fill_color(col);
            // `Tm` sets the text line matrix absolutely (Td would be relative),
            // so every segment is positioned exactly on the page.
            let x_pt: Pt = Mm(x_mm).into();
            let y_pt: Pt = Mm(baseline_mm).into();
            layer.set_text_matrix(TextMatrix::Raw([1.0, 0.0, 0.0, 1.0, x_pt.0, y_pt.0]));
            layer.write_text(text, &font);
            x_mm += text.chars().map(|c| avg_advance(c, *seg_size)).sum::<f32>() * MM_PER_PT;
        }
    }
    layer.end_text_section();
    Ok(())
}

fn paint_picture(
    layer: &PdfLayerReference,
    b: &BoundingBox,
    pic: &PicInfo,
    slide_h_mm: Mm,
) -> Result<(), Error> {
    use image::GenericImageView;
    let raw = decode_data_uri(&pic.data_uri)?;
    let dyn_img = image::load_from_memory(&raw).map_err(|e| Error::Image(e.to_string()))?;
    let (px_w, px_h) = dyn_img.dimensions();
    let img = Image::from_dynamic_image(&dyn_img);
    let (x, y, _w, _h) = rect_in_mm(b, slide_h_mm);
    let target_w_pt = (b.width_emu as f64 * PT_PER_EMU) as f32;
    let target_h_pt = (b.height_emu as f64 * PT_PER_EMU) as f32;
    img.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(x),
            translate_y: Some(y),
            dpi: Some(72.0),
            scale_x: Some(target_w_pt / px_w as f32),
            scale_y: Some(target_h_pt / px_h as f32),
            ..Default::default()
        },
    );
    Ok(())
}

fn decode_data_uri(data_uri: &str) -> Result<Vec<u8>, Error> {
    let b64 = data_uri
        .rsplit(',')
        .next()
        .ok_or_else(|| Error::Image("malformed data uri".into()))?;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| Error::Image(format!("base64: {e}")))
}

/// Map a PPTX font family onto the closest base-14 family, keeping the
/// bold/italic style.
fn font_for(family: Option<&str>, bold: bool, italic: bool) -> BuiltinFont {
    let f = family.unwrap_or("").to_ascii_lowercase();
    let mono = f.contains("courier") || f.contains("mono") || f.contains("consolas");
    let serif = f.contains("times") || f.contains("georgia") || f.contains("garamond");
    if mono {
        match (bold, italic) {
            (true, true) => BuiltinFont::CourierBoldOblique,
            (true, false) => BuiltinFont::CourierBold,
            (false, true) => BuiltinFont::CourierOblique,
            (false, false) => BuiltinFont::Courier,
        }
    } else if serif {
        match (bold, italic) {
            (true, true) => BuiltinFont::TimesBoldItalic,
            (true, false) => BuiltinFont::TimesBold,
            (false, true) => BuiltinFont::TimesItalic,
            (false, false) => BuiltinFont::TimesRoman,
        }
    } else {
        match (bold, italic) {
            (true, true) => BuiltinFont::HelveticaBoldOblique,
            (true, false) => BuiltinFont::HelveticaBold,
            (false, true) => BuiltinFont::HelveticaOblique,
            (false, false) => BuiltinFont::Helvetica,
        }
    }
}

/// Parse the CSS color values produced by `inspect::fill_to_css` /
/// `color_to_css` into 0..=1 RGB components. Returns `None` for tokens the
/// exporter does not draw ("none", "gradient", "pattern", "image") and for
/// names it does not recognize.
fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(s.len() / 2);
    let mut pending: Option<u8> = None;
    for c in s.chars() {
        let d = c.to_digit(16)? as u8;
        match pending.take() {
            Some(hi) => out.push(hi << 4 | d),
            None => pending = Some(d),
        }
    }
    if pending.is_some() {
        return None;
    }
    Some(out)
}

fn parse_css_color(css: &str) -> Option<(f32, f32, f32)> {
    let css = css.trim();
    if let Some(hex) = css.strip_prefix('#') {
        let hex = hex.trim();
        let expanded: String = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect()
        } else if hex.len() == 6 || hex.len() == 8 {
            hex.to_string()
        } else {
            return None;
        };
        let bytes = hex_bytes(&expanded)?;
        return Some((
            bytes[0] as f32 / 255.0,
            bytes[1] as f32 / 255.0,
            bytes[2] as f32 / 255.0,
        ));
    }
    if let Some(rest) = css.strip_prefix("hsl(") {
        let rest = rest.strip_suffix(')').unwrap_or(rest);
        let mut parts = rest
            .split(',')
            .map(|p| p.trim().trim_end_matches('%').parse::<f32>().ok());
        let h = parts.next().flatten()?;
        let s = parts.next().flatten()?;
        let l = parts.next().flatten()?;
        return Some(hsl_to_rgb(h, s / 100.0, l / 100.0));
    }
    named_color(css)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - ((hp.rem_euclid(2.0) - 1.0).abs()));
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r1 + m, g1 + m, b1 + m)
}

const NAMED_COLORS: &[(&str, &str)] = &[
    ("black", "000000"),
    ("white", "ffffff"),
    ("red", "ff0000"),
    ("green", "008000"),
    ("lime", "00ff00"),
    ("blue", "0000ff"),
    ("yellow", "ffff00"),
    ("orange", "ffa500"),
    ("purple", "800080"),
    ("brown", "a52a2a"),
    ("gray", "808080"),
    ("grey", "808080"),
    ("silver", "c0c0c0"),
    ("maroon", "800000"),
    ("olive", "808000"),
    ("navy", "000080"),
    ("teal", "008080"),
    ("aqua", "00ffff"),
    ("cyan", "00ffff"),
    ("magenta", "ff00ff"),
    ("fuchsia", "ff00ff"),
    ("coral", "ff7f50"),
    ("salmon", "fa8072"),
    ("gold", "ffd700"),
    ("indigo", "4b0082"),
    ("violet", "ee82ee"),
    ("beige", "f5f5dc"),
    ("tan", "d2b48c"),
    ("khaki", "f0e68c"),
    ("ivory", "fffff0"),
    ("snow", "fffafa"),
    ("chocolate", "d2691e"),
    ("crimson", "dc143c"),
    ("tomato", "ff6347"),
    ("plum", "dda0dd"),
    ("orchid", "da70d6"),
    ("steelblue", "4682b4"),
    ("skyblue", "87ceeb"),
    ("slategray", "708090"),
    ("slategrey", "708090"),
    ("darkred", "8b0000"),
    ("darkgreen", "006400"),
    ("darkblue", "00008b"),
    ("darkgray", "a9a9a9"),
    ("darkgrey", "a9a9a9"),
    ("lightgray", "d3d3d3"),
    ("lightgrey", "d3d3d3"),
    ("lightblue", "add8e6"),
    ("lightgreen", "90ee90"),
    ("lightyellow", "ffffe0"),
    ("lightpink", "ffb6c1"),
    ("whitesmoke", "f5f5f5"),
    ("gainsboro", "dcdcdc"),
    ("dimgray", "696969"),
    ("dimgrey", "696969"),
    ("windowtext", "000000"),
    ("window", "ffffff"),
    ("activetext", "000000"),
    ("background", "ffffff"),
];

fn named_color(name: &str) -> Option<(f32, f32, f32)> {
    let lower = name.to_ascii_lowercase().replace(' ', "");
    NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, h)| {
            let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f32 / 255.0;
            (b(0), b(2), b(4))
        })
}

fn css_to_pdf_color(css: &str) -> Option<Color> {
    parse_css_color(css).map(|(r, g, b)| {
        Color::Rgb(Rgb::new(
            r.clamp(0.0, 1.0),
            g.clamp(0.0, 1.0),
            b.clamp(0.0, 1.0),
            None,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::TextRunInfo;

    fn run(text: &str, para: usize, size: Option<f64>) -> TextRunInfo {
        TextRunInfo {
            paragraph: para,
            text: text.into(),
            bold: false,
            italic: false,
            font_size_pt: size,
            font_family: None,
            color: None,
            alignment: None,
        }
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(
            parse_css_color("#1A2B3C"),
            Some((
                0x1a as f32 / 255.0,
                0x2b as f32 / 255.0,
                0x3c as f32 / 255.0
            ))
        );
        assert_eq!(parse_css_color("#fff"), Some((1.0, 1.0, 1.0)));
        assert_eq!(parse_css_color("#000000ff"), Some((0.0, 0.0, 0.0)));
        assert_eq!(parse_css_color("none"), None);
        assert_eq!(parse_css_color("gradient"), None);
        assert_eq!(parse_css_color("#zzz"), None);
    }

    #[test]
    fn parses_named_and_hsl() {
        assert_eq!(parse_css_color("red"), Some((1.0, 0.0, 0.0)));
        let (r, g, b) = parse_css_color("hsl(120, 100%, 50%)").unwrap();
        assert!((r - 0.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn font_mapping_covers_styles() {
        assert!(matches!(
            font_for(None, false, false),
            BuiltinFont::Helvetica
        ));
        assert!(matches!(
            font_for(Some("Calibri"), true, false),
            BuiltinFont::HelveticaBold
        ));
        assert!(matches!(
            font_for(Some("Times New Roman"), false, true),
            BuiltinFont::TimesItalic
        ));
        assert!(matches!(
            font_for(Some("Courier New"), true, true),
            BuiltinFont::CourierBoldOblique
        ));
    }

    #[test]
    fn line_breaks_split_paragraphs() {
        let mut r1 = run("abc", 0, Some(24.0));
        r1.alignment = Some("center".into());
        let r2 = run("\n", 0, Some(24.0));
        let mut r3 = run("def", 1, Some(12.0));
        r3.bold = true;
        r3.color = Some("#ff0000".into());
        let lines = build_lines(&[r1, r2, r3]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].segs[0].0, "abc");
        assert_eq!(lines[0].align, "center");
        assert_eq!(lines[1].segs[0].0, "def");
        assert!(lines[1].segs[0].1);
        assert_eq!(lines[1].size_pt, 12.0);
    }
}
