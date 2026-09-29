//! Placeholder geometry inheritance: slide → slide layout → slide master.
//!
//! A placeholder shape on a slide may omit its `<p:spPr>` entirely and inherit
//! its position and size from the matching placeholder in the slide's layout,
//! or — when the layout's own placeholder also omits geometry — from the
//! matching placeholder in the slide master. This is how real PowerPoint decks
//! (and every deck produced programmatically, e.g. by python-pptx) place their
//! titles: the slide carries only the text, the master carries the box.
//!
//! office-toolkit's model does not retain the layout/master parts, so this
//! module re-derives the inherited geometry directly from the original OPC
//! package. Parsing is deliberately narrow: it scans for the specific
//! elements it needs (`<p:sp>` placeholders, their `<a:xfrm>`, and the
//! master's `<p:txStyles>` default sizes) rather than building a general XML
//! tree, and any part it cannot understand simply yields no geometry — the
//! deck then renders exactly as it did before this module existed.

use std::collections::HashMap;

use office_toolkit::powerpoint::PlaceholderKind;
use opc_ooxml::Package;

use crate::inspect::BoundingBox;

/// A placeholder's identity in a layout or master: its `<p:ph type>` token
/// (defaulting to `"body"` when the attribute is absent) and `idx`
/// (defaulting to `0`).
pub type PhKey = (String, u32);

/// Geometry (and default text size) inherited from one layout/master
/// placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhGeom {
    pub bounds: BoundingBox,
    /// Default text size in 1/100 of a point, from the master's
    /// `<p:txStyles>` (`None` when the master declares no size for this
    /// placeholder kind).
    pub font_size_100ths_pt: Option<i32>,
}

/// Resolved placeholder table for one slide: the master's entries with the
/// slide's layout layered on top (a layout entry wins over the master's for
/// the same key).
pub type PhMap = HashMap<PhKey, PhGeom>;

/// Inherited placeholder geometry for every slide of the original file, in
/// file slide order.
#[derive(Debug, Clone, Default)]
pub struct LayoutGeometry {
    by_ordinal: Vec<PhMap>,
}

impl LayoutGeometry {
    /// Resolves the slide → layout → master chain for every slide in the
    /// package, in the presentation's display order.
    pub fn from_package(pkg: &Package) -> Self {
        let mut by_ordinal = Vec::new();
        if let Some(pres_part) = pkg.part("/ppt/presentation.xml") {
            for slide_name in slide_part_order(pres_part) {
                by_ordinal.push(resolve_slide_map(pkg, &slide_name));
            }
        }
        Self { by_ordinal }
    }

    /// Consumes the resolver, yielding the per-ordinal tables for storage.
    pub fn into_maps(self) -> Vec<PhMap> {
        self.by_ordinal
    }

    /// The resolved table for original file slide `ordinal`, when non-empty.
    pub fn for_ordinal(&self, ordinal: usize) -> Option<&PhMap> {
        self.by_ordinal.get(ordinal).filter(|m| !m.is_empty())
    }

    /// Number of slides the original file declared.
    pub fn len(&self) -> usize {
        self.by_ordinal.len()
    }

    /// True when the package declared no slides (or none were resolvable).
    pub fn is_empty(&self) -> bool {
        self.by_ordinal.is_empty()
    }
}

/// The slide part names in display order: `<p:sldIdLst>`'s `r:id` sequence
/// mapped through the presentation part's own relationships.
fn slide_part_order(pres_part: &opc_ooxml::Part) -> Vec<String> {
    let xml = match std::str::from_utf8(&pres_part.data) {
        Ok(x) => x,
        Err(_) => return Vec::new(),
    };
    let Some(open) = xml.find("<p:sldIdLst>") else {
        return Vec::new();
    };
    let Some(close) = xml[open..].find("</p:sldIdLst>") else {
        return Vec::new();
    };
    let list = &xml[open..open + close];

    let mut names = Vec::new();
    let mut rest = list;
    while let Some(i) = rest.find(" r:id=\"") {
        let value_start = i + " r:id=\"".len();
        let Some(quote) = rest[value_start..].find('"') else {
            break;
        };
        let id = &rest[value_start..value_start + quote];
        if let Some(rel) = pres_part.relationships.by_id(id) {
            if let Some(name) = resolve_part("/ppt", &rel.target) {
                names.push(name);
            }
        }
        rest = &rest[value_start + quote + 1..];
    }
    names
}

/// Resolves one slide's placeholder table: its layout's table layered over
/// its master's.
fn resolve_slide_map(pkg: &Package, slide_name: &str) -> PhMap {
    let Some(slide_part) = pkg.part(slide_name) else {
        return PhMap::new();
    };
    let Some(layout_name) = rel_target_of(slide_part, "/ppt/slides", "slideLayout") else {
        return PhMap::new();
    };
    let Some(layout_part) = pkg.part(&layout_name) else {
        return PhMap::new();
    };

    let master_part =
        rel_target_of(layout_part, "/ppt/slideLayouts", "slideMaster").and_then(|m| pkg.part(&m));

    let sizes = master_part
        .map(part_str)
        .map(|x| tx_style_sizes(&x))
        .unwrap_or_default();

    let mut map = PhMap::new();
    if let Some(master) = &master_part {
        if let Ok(xml) = std::str::from_utf8(&master.data) {
            for (key, bounds) in ph_table(xml) {
                map.insert(
                    key.clone(),
                    PhGeom {
                        bounds,
                        font_size_100ths_pt: size_for_type(&sizes, &key.0),
                    },
                );
            }
        }
    }
    if let Ok(xml) = std::str::from_utf8(&layout_part.data) {
        for (key, bounds) in ph_table(xml) {
            let size = size_for_type(&sizes, &key.0);
            map.insert(
                key,
                PhGeom {
                    bounds,
                    font_size_100ths_pt: size,
                },
            );
        }
    }
    map
}

/// The internal relationship target of kind `suffix` (e.g. `"slideLayout"`),
/// resolved to an absolute part name.
fn rel_target_of(part: &opc_ooxml::Part, base_dir: &str, suffix: &str) -> Option<String> {
    let rel = part.relationships.iter().find(|r| {
        r.rel_type.ends_with(suffix) && r.target_mode == opc_ooxml::TargetMode::Internal
    })?;
    resolve_part(base_dir, &rel.target)
}

/// Resolves a relationship target (relative to `base_dir`, which ends
/// without a slash, e.g. `"/ppt/slides"`) into an absolute part name.
fn resolve_part(base_dir: &str, target: &str) -> Option<String> {
    if target.starts_with('/') {
        return Some(target.to_string());
    }
    if target.contains("://") || target.starts_with('#') {
        return None; // external or fragment-only
    }
    let mut out: Vec<&str> = base_dir
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            other => out.push(other),
        }
    }
    Some(format!("/{}", out.join("/")))
}

/// Scans a slide layout/master/slide XML for `<p:sp>` placeholders carrying
/// an explicit offset + extent, keyed by (type token, idx).
fn ph_table(xml: &str) -> Vec<(PhKey, BoundingBox)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<p:sp>") {
        let inner = start + "<p:sp>".len();
        let Some(end_rel) = rest[inner..].find("</p:sp>") else {
            break;
        };
        let seg = &rest[inner..inner + end_rel];
        if let Some(key) = ph_key(seg) {
            if let Some(bounds) = xfrm_box(seg) {
                out.push((key, bounds));
            }
        }
        rest = &rest[inner + end_rel..];
    }
    out
}

/// The (type, idx) of the segment's `<p:ph>` element, if it has one.
fn ph_key(seg: &str) -> Option<PhKey> {
    let start = seg.find("<p:ph")?;
    let tag_end = seg[start..].find('>')?;
    let tag = &seg[start..start + tag_end];
    let ty = attr(tag, "type").unwrap_or("body").to_string();
    let idx = attr(tag, "idx").and_then(|v| v.parse().ok()).unwrap_or(0);
    Some((ty, idx))
}

/// The first `<a:xfrm>` in the segment, when it carries both an offset and
/// an extent.
fn xfrm_box(seg: &str) -> Option<BoundingBox> {
    let start = seg.find("<a:xfrm")?;
    let after = &seg[start..];
    let end = if after.starts_with("<a:xfrm/>") {
        "<a:xfrm/>".len()
    } else {
        after
            .find("</a:xfrm>")
            .map(|i| i + "</a:xfrm>".len())
            .or_else(|| after.find('>').map(|i| i + 1))?
    };
    let xf = &seg[start..start + end];
    let off_start = xf.find("<a:off")?;
    let off_tag = &xf[off_start..off_start + xf[off_start..].find('>')? + 1];
    let x: i64 = attr(off_tag, "x")?.parse().ok()?;
    let y: i64 = attr(off_tag, "y")?.parse().ok()?;
    let ext_start = xf.find("<a:ext")?;
    let ext_tag = &xf[ext_start..ext_start + xf[ext_start..].find('>')? + 1];
    let cx: i64 = attr(ext_tag, "cx")?.parse().ok()?;
    let cy: i64 = attr(ext_tag, "cy")?.parse().ok()?;
    Some(BoundingBox {
        x_emu: x,
        y_emu: y,
        width_emu: cx,
        height_emu: cy,
    })
}

/// The `<p:txStyles>` default sizes: style name → `sz` (1/100 point) of the
/// first level's `<a:defRPr>`.
fn tx_style_sizes(xml: &str) -> HashMap<String, i32> {
    let mut out = HashMap::new();
    for style in ["titleStyle", "bodyStyle", "otherStyle"] {
        let open_tag = format!("<p:{style}>");
        let close_tag = format!("</p:{style}>");
        let Some(open) = xml.find(&open_tag) else {
            continue;
        };
        let Some(close) = xml[open..].find(&close_tag) else {
            continue;
        };
        let block = &xml[open..open + close];
        let Some(lvl_open) = block.find("<a:lvl1pPr") else {
            continue;
        };
        let lvl_end = block[lvl_open..]
            .find("</a:lvl1pPr>")
            .map(|i| lvl_open + i + "</a:lvl1pPr>".len())
            .unwrap_or(block.len());
        let lvl = &block[lvl_open..lvl_end];
        let Some(def_start) = lvl.find("<a:defRPr") else {
            continue;
        };
        let Some(gt) = lvl[def_start..].find('>') else {
            continue;
        };
        let def_tag = &lvl[def_start..def_start + gt + 1];
        if let Some(sz) = attr(def_tag, "sz").and_then(|v| v.parse().ok()) {
            out.insert(style.to_string(), sz);
        }
    }
    out
}

/// Which `<p:txStyles>` style governs a placeholder type.
fn size_for_type(sizes: &HashMap<String, i32>, ty: &str) -> Option<i32> {
    let style = match ty {
        "title" | "ctrTitle" => "titleStyle",
        "body" | "subTitle" => "bodyStyle",
        _ => "otherStyle",
    };
    sizes
        .get(style)
        .copied()
        .or_else(|| sizes.get("bodyStyle").copied())
}

/// The XML `type` token for a model placeholder kind.
pub fn xml_token(kind: &PlaceholderKind) -> String {
    match kind {
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
    .to_string()
}

fn part_str(part: &opc_ooxml::Part) -> String {
    String::from_utf8_lossy(&part.data).into_owned()
}

/// Reads `name="value"` from a start tag's attribute list.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let i = tag.find(&needle)?;
    let rest = &tag[i + needle.len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>
<p:txStyles><p:titleStyle><a:lvl1pPr algn="l"><a:defRPr sz="4400"/></a:lvl1pPr></p:titleStyle>
<p:bodyStyle><a:lvl1pPr><a:defRPr sz="2400"/></a:lvl1pPr></p:bodyStyle></p:txStyles>
</p:sldMaster>"##;

    #[test]
    fn ph_table_reads_placeholders_with_geometry() {
        let xml = r##"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="457200" y="274638"/><a:ext cx="8229600" cy="1143000"/></a:xfrm></p:spPr></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body 2"/><p:nvPr><p:ph type="body" idx="2"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="4" name="Body 3"/><p:nvPr><p:ph idx="3"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100" y="200"/><a:ext cx="300" cy="400"/></a:xfrm></p:spPr></p:sp>"##;
        let table = ph_table(xml);
        assert_eq!(table.len(), 2);
        assert_eq!(
            table[0],
            (
                ("title".to_string(), 0),
                BoundingBox {
                    x_emu: 457_200,
                    y_emu: 274_638,
                    width_emu: 8_229_600,
                    height_emu: 1_143_000
                }
            )
        );
        // A placeholder with no xfrm is skipped.
        assert_eq!(table[1].0, ("body".to_string(), 3u32));
    }

    #[test]
    fn tx_style_sizes_reads_first_level_default_run() {
        let sizes = tx_style_sizes(MASTER);
        assert_eq!(sizes.get("titleStyle"), Some(&4400));
        assert_eq!(sizes.get("bodyStyle"), Some(&2400));
        assert_eq!(size_for_type(&sizes, "title"), Some(4400));
        assert_eq!(size_for_type(&sizes, "subTitle"), Some(2400));
        // No otherStyle → falls back to bodyStyle.
        assert_eq!(size_for_type(&sizes, "ftr"), Some(2400));
    }

    #[test]
    fn resolve_part_handles_relative_targets() {
        assert_eq!(
            resolve_part("/ppt/slides", "../slideLayouts/slideLayout6.xml").as_deref(),
            Some("/ppt/slideLayouts/slideLayout6.xml")
        );
        assert_eq!(
            resolve_part("/ppt", "slides/slide1.xml").as_deref(),
            Some("/ppt/slides/slide1.xml")
        );
        assert_eq!(resolve_part("/ppt", "http://x.example/a").as_deref(), None);
    }

    #[test]
    fn attr_reads_named_attributes() {
        let tag = r#"<p:ph type="body" idx="7" sz="half"/>"#;
        assert_eq!(attr(tag, "type"), Some("body"));
        assert_eq!(attr(tag, "idx"), Some("7"));
        assert_eq!(attr(tag, "sz"), Some("half"));
        assert_eq!(attr(tag, "nope"), None);
    }
}
