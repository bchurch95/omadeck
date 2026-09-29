use office_toolkit::drawing::{
    ShapeProperties, TextBody, TextParagraph, TextRun, TextRunProperties, Transform2D,
};
use office_toolkit::powerpoint::{
    AutoShape, Placeholder, PlaceholderKind, Presentation, Shape, Slide,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresentationModel {
    pub title: String,
    pub slides: Vec<SlideModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlideModel {
    pub index: usize,
    pub title: Option<String>,
    pub notes: Option<String>,
}

impl PresentationModel {
    pub fn from_presentation(pres: Presentation) -> Self {
        Self::from_presentation_ref(&pres)
    }

    /// Non-consuming view of a full deck — keeps every shape intact in the original.
    pub fn from_presentation_ref(pres: &Presentation) -> Self {
        let title = pres
            .properties
            .title
            .clone()
            .unwrap_or_else(|| "Untitled".to_string());
        let slides = pres
            .slides
            .iter()
            .enumerate()
            .map(|(i, slide)| {
                let title_text = extract_slide_title(slide);
                let notes_text = slide.notes.as_ref().map(text_body_to_string);
                SlideModel {
                    index: i,
                    title: if title_text.trim().is_empty() {
                        None
                    } else {
                        Some(title_text)
                    },
                    notes: if notes_text.as_deref().unwrap_or("").trim().is_empty() {
                        None
                    } else {
                        notes_text
                    },
                }
            })
            .collect();
        Self { title, slides }
    }

    pub fn to_presentation(&self) -> Presentation {
        let mut pres = Presentation::new();
        for slide in &self.slides {
            let mut s = Slide::new();
            if let Some(title) = &slide.title {
                if !title.trim().is_empty() {
                    s.add_shape(Shape::AutoShape(
                        AutoShape::new(1, "Title")
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
                    ));
                }
            }
            if let Some(notes) = &slide.notes {
                if !notes.trim().is_empty() {
                    s.notes = Some(text_body_from_string(notes));
                }
            }
            pres.slides.push(s);
        }
        if !self.title.trim().is_empty() && self.title != "Untitled" {
            pres.properties.title = Some(self.title.clone());
        }
        pres
    }
}

pub(crate) fn text_body_from_string(text: &str) -> TextBody {
    let mut body = TextBody::new();
    for line in text.lines() {
        let para = TextParagraph::new().with_run(TextRun::Regular {
            text: line.to_string(),
            properties: TextRunProperties::new(),
        });
        body = body.with_paragraph(para);
    }
    body
}

fn extract_slide_title(slide: &Slide) -> String {
    for shape in &slide.shapes {
        if let Shape::AutoShape(auto) = shape {
            // Prefer placeholder title shapes
            if let Some(ph) = &auto.placeholder {
                use office_toolkit::powerpoint::PlaceholderKind;
                if matches!(
                    ph.kind,
                    PlaceholderKind::Title | PlaceholderKind::CenterTitle
                ) {
                    if let Some(tb) = &auto.text_body {
                        return text_body_to_string(tb);
                    }
                }
            }
        }
    }
    // Fallback: first non-empty text body
    for shape in &slide.shapes {
        if let Shape::AutoShape(auto) = shape {
            if let Some(tb) = &auto.text_body {
                let s = text_body_to_string(tb);
                if !s.trim().is_empty() {
                    return s;
                }
            }
        }
    }
    String::new()
}

pub(crate) fn text_body_to_string(tb: &TextBody) -> String {
    let mut out = String::new();
    for para in &tb.paragraphs {
        let mut line = String::new();
        for run in &para.runs {
            let txt = match run {
                TextRun::Regular { text, .. } => text.clone(),
                TextRun::LineBreak { .. } => "\n".to_string(),
                TextRun::Field { cached_text, .. } => cached_text.clone(),
            };
            line.push_str(&txt);
        }
        if !line.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&line);
        }
    }
    out
}
