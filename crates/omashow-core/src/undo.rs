//! Command-pattern undo/redo for deck mutations.
//!
//! Every mutation command snapshots exactly the state it touched — a slide-list
//! suffix for add/delete/reorder, one slide's shapes for content edits, one
//! slide's notes — rather than the whole deck, so stack memory stays
//! proportional to edit size. Each command carries both directions: `apply`
//! re-plays the change (redo) and `revert` restores the captured state (undo).

use office_toolkit::drawing::TextBody;
use office_toolkit::powerpoint::{Presentation, Shape, Slide};

/// A single reversible deck mutation.
#[derive(Debug, Clone)]
pub enum UndoCommand {
    /// The slide list from `from` onward, captured before and after the change.
    /// Covers slide insert, delete, and reorder. `ord_*` mirrors the tail of
    /// the document's original-file slide ordinals so per-slide extras (like
    /// inherited placeholder geometry) follow their slides through undo/redo.
    Slides {
        from: usize,
        before: Vec<Slide>,
        after: Vec<Slide>,
        ord_before: Vec<Option<usize>>,
        ord_after: Vec<Option<usize>>,
        description: String,
    },
    /// One slide's shape tree, captured before and after.
    Shapes {
        slide: usize,
        before: Vec<Shape>,
        after: Vec<Shape>,
        description: String,
    },
    /// One slide's speaker notes, captured before and after.
    Notes {
        slide: usize,
        before: Option<TextBody>,
        after: Option<TextBody>,
        description: String,
    },
    /// One slide's explicit background fill, captured before and after.
    Background {
        slide: usize,
        before: Option<office_toolkit::drawing::Fill>,
        after: Option<office_toolkit::drawing::Fill>,
        description: String,
    },
}

impl UndoCommand {
    /// Re-plays the command (the redo direction).
    pub fn apply(&self, pres: &mut Presentation) {
        match self {
            UndoCommand::Slides { from, after, .. } => {
                if *from <= pres.slides.len() {
                    pres.slides.splice(*from.., after.iter().cloned());
                }
            }
            UndoCommand::Shapes { slide, after, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.shapes = after.clone();
                }
            }
            UndoCommand::Notes { slide, after, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.notes = after.clone();
                }
            }
            UndoCommand::Background { slide, after, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.background = after.clone();
                }
            }
        }
    }

    /// Restores the captured pre-change state (the undo direction).
    pub fn revert(&self, pres: &mut Presentation) {
        match self {
            UndoCommand::Slides { from, before, .. } => {
                if *from <= pres.slides.len() {
                    pres.slides.splice(*from.., before.iter().cloned());
                }
            }
            UndoCommand::Shapes { slide, before, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.shapes = before.clone();
                }
            }
            UndoCommand::Notes { slide, before, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.notes = before.clone();
                }
            }
            UndoCommand::Background { slide, before, .. } => {
                if let Some(s) = pres.slides.get_mut(*slide) {
                    s.background = before.clone();
                }
            }
        }
    }

    /// Human-readable label for UI ("Undo: add slide").
    pub fn description(&self) -> &str {
        match self {
            UndoCommand::Slides { description, .. }
            | UndoCommand::Shapes { description, .. }
            | UndoCommand::Notes { description, .. }
            | UndoCommand::Background { description, .. } => description,
        }
    }
}

/// Bounded undo/redo stack over deck mutations.
#[derive(Debug)]
pub struct UndoStack {
    undo: Vec<UndoCommand>,
    redo: Vec<UndoCommand>,
    limit: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: 200,
        }
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a command onto the undo side. Any new command clears the redo side.
    pub fn record(&mut self, command: UndoCommand) {
        self.undo.push(command);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Description of the most recent command, if any — for "Undo: …" UI labels.
    pub fn last_description(&self) -> Option<&str> {
        self.undo.last().map(|c| c.description())
    }

    /// Undoes the most recent command, applying its revert direction.
    /// Returns the executed command so callers can mirror its slide-list
    /// change into their own per-slide bookkeeping.
    pub fn undo(&mut self, pres: &mut Presentation) -> Option<UndoCommand> {
        let command = self.undo.pop()?;
        command.revert(pres);
        self.redo.push(command.clone());
        Some(command)
    }

    /// Re-applies the most recently undone command. Returns the executed
    /// command (see [`Self::undo`]).
    pub fn redo(&mut self, pres: &mut Presentation) -> Option<UndoCommand> {
        let command = self.redo.pop()?;
        command.apply(pres);
        self.undo.push(command.clone());
        Some(command)
    }

    /// Clears both sides (e.g. when a new deck replaces the current one).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Number of undo steps available.
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_toolkit::powerpoint::PlaceholderKind;

    fn slide_with_title(title: &str) -> Slide {
        use office_toolkit::drawing::{ShapeProperties, Transform2D};
        use office_toolkit::powerpoint::{AutoShape, Placeholder, Shape};
        use crate::model::text_body_from_string;
        let mut s = Slide::new();
        s.shapes.push(Shape::AutoShape(
            AutoShape::new(2, "Title")
                .with_placeholder(Placeholder::new(PlaceholderKind::Title))
                .with_properties(
                    ShapeProperties::new().with_transform(
                        Transform2D::new().with_offset(685_800, 342_900).with_extent(10_820_400, 1_325_555),
                    ),
                )
                .with_text_body(text_body_from_string(title)),
        ));
        s
    }

    fn titles(pres: &Presentation) -> Vec<String> {
        pres.slides
            .iter()
            .map(|s| s.shapes.iter().find_map(|sh| match sh {
                Shape::AutoShape(a) => a.text_body.as_ref(),
                _ => None,
            }).map(crate::model::text_body_to_string).unwrap_or_default())
            .collect()
    }

    #[test]
    fn undo_redo_slide_insertion_roundtrip() {
        let mut pres = Presentation::new();
        pres.slides.push(slide_with_title("A"));
        let mut stack = UndoStack::new();

        let new_slide = slide_with_title("B");
        let before = Vec::new();
        let after = vec![new_slide.clone()];
        stack.record(UndoCommand::Slides {
            from: 1,
            before,
            after: after.clone(),
            ord_before: Vec::new(),
            ord_after: vec![None],
            description: "add slide".into(),
        });
        pres.slides.push(new_slide);
        assert_eq!(titles(&pres), vec!["A", "B"]);

        let undone = stack.undo(&mut pres).expect("undo works");
        assert_eq!(undone.description(), "add slide");
        assert_eq!(titles(&pres), vec!["A"]);
        assert!(stack.can_redo() && !stack.can_undo());

        let redone = stack.redo(&mut pres).expect("redo works");
        assert_eq!(redone.description(), "add slide");
        assert_eq!(titles(&pres), vec!["A", "B"]);
        assert!(stack.can_undo() && !stack.can_redo());
    }

    #[test]
    fn undo_slide_reorder_restores_order() {
        let mut pres = Presentation::new();
        pres.slides.push(slide_with_title("A"));
        pres.slides.push(slide_with_title("B"));
        pres.slides.push(slide_with_title("C"));
        let mut stack = UndoStack::new();

        let before = pres.slides.clone();
        let mut moved = before.clone();
        let c = moved.remove(2);
        moved.insert(0, c);
        pres.slides = moved;
        assert_eq!(titles(&pres), vec!["C", "A", "B"]);
        stack.record(UndoCommand::Slides {
            from: 0,
            before,
            after: pres.slides.clone(),
            ord_before: vec![Some(0), Some(1), Some(2)],
            ord_after: vec![Some(2), Some(0), Some(1)],
            description: "move slide".into(),
        });

        stack.undo(&mut pres).unwrap();
        assert_eq!(titles(&pres), vec!["A", "B", "C"]);
    }

    #[test]
    fn new_command_clears_redo_side() {
        let mut pres = Presentation::new();
        pres.slides.push(slide_with_title("A"));
        let mut stack = UndoStack::new();
        stack.record(UndoCommand::Notes { slide: 0, before: None, after: None, description: "n1".into() });
        assert!(stack.can_undo());
        stack.undo(&mut pres).unwrap();
        assert!(stack.can_redo());

        stack.record(UndoCommand::Notes { slide: 0, before: None, after: None, description: "n2".into() });
        assert!(!stack.can_redo(), "a new command must drop the redo side");
    }

    #[test]
    fn shapes_command_restores_shape_tree() {
        let mut pres = Presentation::new();
        pres.slides.push(slide_with_title("A"));
        let mut stack = UndoStack::new();

        let before = pres.slides[0].shapes.clone();
        stack.record(UndoCommand::Shapes { slide: 0, before: before.clone(), after: Vec::new(), description: "clear shapes".into() });
        pres.slides[0].shapes.clear();
        assert!(pres.slides[0].shapes.is_empty());

        stack.undo(&mut pres).unwrap();
        assert_eq!(pres.slides[0].shapes.len(), before.len());
    }
}
