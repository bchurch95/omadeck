use office_toolkit::drawing::{
    ShapeProperties, TextBody, TextParagraph, TextParagraphProperties, TextRun, TextRunProperties,
    Transform2D,
};
use office_toolkit::powerpoint::{AutoShape, Placeholder, PlaceholderKind, Shape, Slide};
use office_toolkit::{OpenFile, SaveToFile};

use model::text_body_from_string;

pub mod document;
pub mod error;
pub mod export_html;
pub mod export_pdf;
pub mod inspect;
pub mod io;
pub mod layout_geom;
pub mod model;
pub mod morph;
pub mod undo;

pub use document::PptxDocument;
pub use error::Error;
pub use inspect::{
    get_slide_shapes, slide_background, slide_count, slide_dimensions, BoundingBox, LineInfo,
    ShapeInfo, SlideDimensions, TextRunInfo,
};
pub use layout_geom::{LayoutGeometry, PhGeom, PhMap};
pub use model::{PresentationModel, SlideModel};
pub use morph::{detect_morph, MorphBox, MorphMatch, MorphResult};
pub use office_toolkit::powerpoint::Presentation;
pub use undo::{UndoCommand, UndoStack};

/// Open a PPTX file into a lightweight model (title + per-slide title/notes).
pub fn open_pptx(path: &str) -> Result<PresentationModel, Error> {
    let pres = Presentation::open_file(path).map_err(Error::OfficeToolkit)?;
    Ok(PresentationModel::from_presentation(pres))
}

/// Open a PPTX file into the *full* deck, preserving every shape/picture/chart.
pub fn open_pptx_full(path: &str) -> Result<Presentation, Error> {
    Presentation::open_file(path).map_err(Error::OfficeToolkit)
}

/// Save a full deck to PPTX, losslessly.
pub fn save_presentation(path: &str, pres: &Presentation) -> Result<(), Error> {
    pres.save_to_file(path).map_err(Error::OfficeToolkit)
}

/// Save a lightweight model to PPTX (rebuilds the deck; use `save_presentation` for lossless saves).
pub fn save_pptx(path: &str, model: &PresentationModel) -> Result<(), Error> {
    let pres = model.to_presentation();
    save_presentation(path, &pres)
}

/// Lightweight model view of a deck, without consuming it.
pub fn model_of(pres: &Presentation) -> PresentationModel {
    PresentationModel::from_presentation_ref(pres)
}

/// Set the title of a slide in place — edits the existing title placeholder if present,
/// otherwise inserts a standard title text box. All other content is untouched.
pub fn set_slide_title(pres: &mut Presentation, slide: usize, title: &str) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let title = title.to_string();

    // 1) Prefer an existing title/centerTitle placeholder shape.
    let idx = slide
        .shapes
        .iter()
        .position(|shape| matches!(shape, Shape::AutoShape(a) if a.placeholder.as_ref().is_some_and(|p| matches!(p.kind, PlaceholderKind::Title | PlaceholderKind::CenterTitle))));
    if let Some(i) = idx {
        if let Shape::AutoShape(auto) = &mut slide.shapes[i] {
            auto.text_body = Some(text_body_from_string(&title));
            return Ok(());
        }
    }

    // 2) Fallback: repurpose the first shape carrying text.
    let idx = slide
        .shapes
        .iter()
        .position(|shape| matches!(shape, Shape::AutoShape(a) if a.text_body.as_ref().is_some_and(|tb| !tb_text_is_empty(tb))));
    if let Some(i) = idx {
        if let Shape::AutoShape(auto) = &mut slide.shapes[i] {
            auto.text_body = Some(text_body_from_string(&title));
            return Ok(());
        }
    }

    // 3) No title shape at all — insert a standard one.
    slide.shapes.push(title_shape(&title, next_shape_id(slide)));
    Ok(())
}

/// Set (or clear, with None) the speaker notes of a slide in place.
pub fn set_slide_notes(
    pres: &mut Presentation,
    slide: usize,
    notes: Option<String>,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    slide.notes = match notes {
        Some(n) if !n.trim().is_empty() => Some(text_body_from_string(&n)),
        _ => None,
    };
    Ok(())
}

/// Set (or clear, with None) a slide's explicit background fill in place.
pub fn set_slide_background(
    pres: &mut Presentation,
    slide: usize,
    fill: Option<office_toolkit::drawing::Fill>,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    slide.background = fill;
    Ok(())
}

/// Append a new empty slide with a standard title text box.
pub fn add_slide(pres: &mut Presentation, title: Option<String>) -> Result<usize, Error> {
    let mut s = Slide::new();
    if let Some(title) = title {
        if !title.trim().is_empty() {
            s.shapes.push(title_shape(&title, 2));
        }
    }
    pres.slides.push(s);
    Ok(pres.slides.len() - 1)
}

/// Remove a slide from the deck in place.
pub fn delete_slide(pres: &mut Presentation, slide: usize) -> Result<(), Error> {
    if slide >= pres.slides.len() {
        return Err(Error::OutOfRange(slide));
    }
    pres.slides.remove(slide);
    Ok(())
}

/// Insert a new slide with an optional title at `index`. An index one past the
/// end appends; anything farther is an error.
pub fn add_slide_at(
    pres: &mut Presentation,
    index: usize,
    title: Option<String>,
) -> Result<usize, Error> {
    if index > pres.slides.len() {
        return Err(Error::OutOfRange(index));
    }
    let mut s = Slide::new();
    if let Some(title) = title {
        if !title.trim().is_empty() {
            s.shapes.push(title_shape(&title, 2));
        }
    }
    pres.slides.insert(index, s);
    Ok(index)
}

/// Move the slide at `from` so it ends up at position `to` in the final order.
/// `to` is interpreted in final-list coordinates, so `move_slide(0, 2)` on a
/// 3-slide deck sends slide 1 to the end.
pub fn move_slide(pres: &mut Presentation, from: usize, to: usize) -> Result<(), Error> {
    if from >= pres.slides.len() {
        return Err(Error::OutOfRange(from));
    }
    if to >= pres.slides.len() {
        return Err(Error::OutOfRange(to));
    }
    if from == to {
        return Ok(());
    }
    let slide = pres.slides.remove(from);
    pres.slides.insert(to.min(pres.slides.len()), slide);
    Ok(())
}

/// Z-order (painting order) operations on a slide's shape tree.
/// Index 0 of `Slide::shapes` is backmost; the last entry is frontmost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZOrderAction {
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
}

/// Reorder one shape within a slide's z-order (document order of `p:spTree`).
/// Returns whether the order actually changed — boundary cases (frontmost
/// shape + `BringForward`, backmost + `SendBackward`, single-shape slides)
/// are no-ops and report `Ok(false)`.
pub fn reorder_shape_z_order(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
    action: ZOrderAction,
) -> Result<bool, Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let idx = slide
        .shapes
        .iter()
        .position(|s| match s {
            Shape::AutoShape(a) => a.id == shape_id,
            Shape::Picture(p) => p.id == shape_id,
            Shape::Chart(c) => c.id == shape_id,
            Shape::Group(g) => g.id == shape_id,
            Shape::Connector(c) => c.id == shape_id,
            Shape::Table(t) => t.id == shape_id,
            Shape::Media(m) => m.id == shape_id,
        })
        .ok_or(Error::ShapeNotFound(shape_id))?;

    let n = slide.shapes.len();
    if n <= 1 {
        return Ok(false);
    }

    let changed = match action {
        ZOrderAction::BringToFront => {
            let shape = slide.shapes.remove(idx);
            slide.shapes.push(shape);
            idx != n - 1
        }
        ZOrderAction::SendToBack => {
            let shape = slide.shapes.remove(idx);
            slide.shapes.insert(0, shape);
            idx != 0
        }
        ZOrderAction::BringForward => {
            if idx + 1 < n {
                slide.shapes.swap(idx, idx + 1);
                true
            } else {
                false
            }
        }
        ZOrderAction::SendBackward => {
            if idx > 0 {
                slide.shapes.swap(idx, idx - 1);
                true
            } else {
                false
            }
        }
    };
    Ok(changed)
}

/// Shift a shape's position by `dx`/`dy` EMU (a delta applied on top of the
/// shape's current offset). Works for every top-level kind that carries a
/// position: AutoShape/Connector via `transform.offset`, and Picture/Chart/
/// Group/Table/Media via `offset_emu`. A missing transform or offset is created
/// so a delta is never silently dropped.
pub fn move_shape(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
    dx: i64,
    dy: i64,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let mut moved = false;
    for shape in slide.shapes.iter_mut() {
        let is_target = match shape {
            Shape::AutoShape(a) => a.id == shape_id,
            Shape::Picture(p) => p.id == shape_id,
            Shape::Chart(c) => c.id == shape_id,
            Shape::Group(g) => g.id == shape_id,
            Shape::Connector(c) => c.id == shape_id,
            Shape::Table(t) => t.id == shape_id,
            Shape::Media(m) => m.id == shape_id,
        };
        if !is_target {
            continue;
        }
        match shape {
            Shape::AutoShape(a) => shift_transform_offset(&mut a.properties, dx, dy),
            Shape::Connector(c) => shift_transform_offset(&mut c.properties, dx, dy),
            Shape::Picture(p) => shift_offset_emu(&mut p.offset_emu, dx, dy),
            Shape::Chart(c) => shift_offset_emu(&mut c.offset_emu, dx, dy),
            Shape::Group(g) => shift_offset_emu(&mut g.offset_emu, dx, dy),
            Shape::Table(t) => shift_offset_emu(&mut t.offset_emu, dx, dy),
            Shape::Media(m) => shift_offset_emu(&mut m.offset_emu, dx, dy),
        }
        moved = true;
        break;
    }
    if moved {
        Ok(())
    } else {
        Err(Error::ShapeNotFound(shape_id))
    }
}

/// Shift an AutoShape/Connector's `transform.offset` by `dx`/`dy` EMU, creating
/// the transform and offset if they are missing so the delta is never lost.
fn shift_transform_offset(props: &mut ShapeProperties, dx: i64, dy: i64) {
    let t = props.transform.get_or_insert_with(Transform2D::new);
    let o = t.offset.get_or_insert((0, 0));
    o.0 = o.0.saturating_add(dx);
    o.1 = o.1.saturating_add(dy);
}

/// Add `dx`/`dy` to an `offset_emu` pair (used by the picture/chart/group/
/// table/media shape kinds).
fn shift_offset_emu(offset: &mut (i64, i64), dx: i64, dy: i64) {
    offset.0 = offset.0.saturating_add(dx);
    offset.1 = offset.1.saturating_add(dy);
}

/// Remove the top-level shape identified by `shape_id` from the slide and
/// return its display name (used for the UI undo hint). The full shape is
/// restored on undo via the history snapshot.
pub fn delete_shape(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
) -> Result<String, Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let idx = slide
        .shapes
        .iter()
        .position(|s| match s {
            Shape::AutoShape(a) => a.id == shape_id,
            Shape::Picture(p) => p.id == shape_id,
            Shape::Chart(c) => c.id == shape_id,
            Shape::Group(g) => g.id == shape_id,
            Shape::Connector(c) => c.id == shape_id,
            Shape::Table(t) => t.id == shape_id,
            Shape::Media(m) => m.id == shape_id,
        })
        .ok_or(Error::ShapeNotFound(shape_id))?;
    let shape = slide.shapes.remove(idx);
    Ok(match shape {
        Shape::AutoShape(a) => a.name,
        Shape::Picture(p) => p.name,
        Shape::Chart(c) => c.name,
        Shape::Group(g) => g.name,
        Shape::Connector(c) => c.name,
        Shape::Table(t) => t.name,
        Shape::Media(m) => m.name,
    })
}

/// Replace the text of a single run at flat index `run_idx` within the shape's
/// text body, preserving every other run and the shape's formatting. Runs are
/// counted in document order across all paragraphs. A `Regular` run gets its
/// `text` replaced; a `Field` run gets its `cached_text` replaced; a
/// `LineBreak` at that index is an error because it carries no text.
pub fn set_text_run(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
    run_idx: usize,
    text: &str,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let target =
        find_autoshape(&mut slide.shapes, shape_id).ok_or(Error::ShapeNotFound(shape_id))?;
    let body = match target.text_body.as_mut() {
        Some(b) => b,
        None => return Err(Error::ShapeNotFound(shape_id)),
    };
    let mut seen = 0usize;
    for para in body.paragraphs.iter_mut() {
        for run in para.runs.iter_mut() {
            if seen != run_idx {
                seen += 1;
                continue;
            }
            return match run {
                TextRun::Regular { text: run_text, .. } => {
                    *run_text = text.to_string();
                    Ok(())
                }
                TextRun::Field { cached_text, .. } => {
                    *cached_text = text.to_string();
                    Ok(())
                }
                TextRun::LineBreak { .. } => Err(Error::OutOfRange(run_idx)),
            };
        }
    }
    Err(Error::OutOfRange(run_idx))
}

/// Replace the text of the shape identified by `shape_id` on `slide`.
/// Searches top-level shapes and one level into groups. The replacement text
/// is laid out as one paragraph per line and inherits the shape's original
/// first-paragraph and first-run formatting (alignment, font, size, color),
/// so editing a run does not flatten the shape's style.
pub fn update_text_run(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
    new_text: &str,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let target =
        find_autoshape(&mut slide.shapes, shape_id).ok_or(Error::ShapeNotFound(shape_id))?;
    let (para_props, run_props) = target
        .text_body
        .as_ref()
        .map(|tb| {
            let para_props = tb.paragraphs.first().and_then(|p| p.properties.clone());
            let run_props = tb.paragraphs.iter().find_map(|p| {
                p.runs.iter().find_map(|r| match r {
                    TextRun::Regular { properties, .. } => Some(properties.clone()),
                    _ => None,
                })
            });
            (para_props, run_props)
        })
        .unwrap_or_default();
    target.text_body = Some(rebuild_text_body(new_text, para_props, run_props));
    Ok(())
}

/// Finds an autoshape by its `cNvPr` id, searching top-level shapes and the
/// direct children of group shapes.
fn find_autoshape(shapes: &mut [Shape], id: u32) -> Option<&mut AutoShape> {
    for shape in shapes.iter_mut() {
        match shape {
            Shape::AutoShape(auto) if auto.id == id => return Some(auto),
            Shape::Group(group) => {
                if let Some(found) = find_autoshape(&mut group.shapes, id) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Builds a text body with one paragraph per line of `text`, reusing the
/// captured paragraph/run properties when present.
fn rebuild_text_body(
    text: &str,
    para_props: Option<TextParagraphProperties>,
    run_props: Option<TextRunProperties>,
) -> TextBody {
    let mut body = TextBody::new();
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };
    for line in lines {
        let mut para = TextParagraph::new();
        if let Some(props) = &para_props {
            para = para.with_properties(props.clone());
        }
        if !line.is_empty() {
            let props = run_props.clone().unwrap_or_default();
            para = para.with_run(TextRun::Regular {
                text: line.to_string(),
                properties: props,
            });
        }
        body = body.with_paragraph(para);
    }
    body
}

/// The standard title text box used when a slide has no title placeholder.
fn title_shape(title: &str, id: u32) -> Shape {
    Shape::AutoShape(
        AutoShape::new(id, "Title")
            .with_text_box(true)
            .with_placeholder(Placeholder::new(PlaceholderKind::Title))
            .with_properties(
                ShapeProperties::new().with_transform(
                    Transform2D::new()
                        .with_offset(685_800, 342_900)
                        .with_extent(10_820_400, 1_325_555),
                ),
            )
            .with_text_body(text_body_from_string(title)),
    )
}

/// Replace the whole deck (e.g. after a "New" action).
pub fn new_presentation() -> Presentation {
    Presentation::new()
}

fn tb_text_is_empty(tb: &TextBody) -> bool {
    tb.paragraphs.iter().all(|p| {
        p.runs.iter().all(|r| match r {
            TextRun::Regular { text, .. } => text.trim().is_empty(),
            _ => true,
        })
    })
}

fn next_shape_id(slide: &Slide) -> u32 {
    let max = slide
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::AutoShape(a) => Some(a.id),
            Shape::Picture(p) => Some(p.id),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    max + 1
}
