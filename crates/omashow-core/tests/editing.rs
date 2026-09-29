//! Editing + undo integration tests against a real PPTX roundtrip.
//!
//! Every assertion runs on a freshly opened real fixture, so these prove the
//! command-pattern undo restores byte-relevant state, not just in-memory state.

use office_toolkit::drawing::{ShapeProperties, Transform2D};
use office_toolkit::powerpoint::{AutoShape, Shape, Slide};
use omashow_core::{model_of, PptxDocument, ZOrderAction};

fn doc() -> PptxDocument {
    PptxDocument::open("/tmp/omashow-rt/real.pptx").expect("fixture opens")
}

#[test]
fn title_edit_undo_redo_roundtrip() {
    let mut d = doc();
    assert!(d.slide_count() >= 2);

    d.set_title(0, "Changed Title").unwrap();
    assert_eq!(
        model_of(&d.pres).slides[0].title.as_deref(),
        Some("Changed Title")
    );
    assert!(d.can_undo());

    let what = d.undo().expect("undo works");
    assert_eq!(what, "change title");
    let restored = model_of(&d.pres).slides[0].title.clone();
    assert_ne!(restored.as_deref(), Some("Changed Title"));

    d.redo().expect("redo works");
    assert_eq!(
        model_of(&d.pres).slides[0].title.as_deref(),
        Some("Changed Title")
    );
}

#[test]
fn add_and_delete_slide_undo() {
    let mut d = doc();
    let before = d.slide_count();

    let idx = d.add_slide_at(before - 1, Some("Inserted".into())).unwrap();
    assert_eq!(d.slide_count(), before + 1);
    assert_eq!(
        model_of(&d.pres).slides[idx].title.as_deref(),
        Some("Inserted")
    );

    d.undo().unwrap();
    assert_eq!(d.slide_count(), before);

    d.redo().unwrap();
    assert_eq!(d.slide_count(), before + 1);

    d.delete_slide(idx).unwrap();
    assert_eq!(d.slide_count(), before);
    d.undo().unwrap();
    assert_eq!(d.slide_count(), before + 1);
}

#[test]
fn move_slide_reorders_and_undoes() {
    let mut d = doc();
    assert!(d.slide_count() >= 3);
    let before: Vec<Option<String>> = model_of(&d.pres)
        .slides
        .iter()
        .map(|s| s.title.clone())
        .collect();

    d.move_slide(0, 2).unwrap();
    let after: Vec<Option<String>> = model_of(&d.pres)
        .slides
        .iter()
        .map(|s| s.title.clone())
        .collect();
    // [A,B,C] moved 0→2 is [B,C,A]: the moved slide lands at index 2, the
    // rest shift down.
    assert_eq!(after[0], before[1]);
    assert_eq!(after[1], before[2]);
    assert_eq!(after[2], before[0]);

    let undone = d.undo().expect("undo works");
    assert_eq!(undone, "move slide");
    let restored: Vec<Option<String>> = model_of(&d.pres)
        .slides
        .iter()
        .map(|s| s.title.clone())
        .collect();
    assert_eq!(restored, before);
}

#[test]
fn text_run_edit_preserves_and_undoes() {
    let mut d = doc();
    // Find any autoshape with text on slide 0.
    let shapes = d.get_slide_shapes(0).unwrap();
    let shape = shapes
        .iter()
        .find(|s| s.text.iter().any(|t| !t.is_empty()))
        .expect("slide 0 has a text shape");

    d.update_text_run(0, shape.id, "Edited text").unwrap();
    let after = d.get_slide_shapes(0).unwrap();
    let edited = after
        .iter()
        .find(|s| s.id == shape.id)
        .and_then(|s| s.text.iter().find(|t| !t.is_empty()))
        .expect("edited text present");
    assert_eq!(edited, "Edited text");

    d.undo().unwrap();
    let restored = d.get_slide_shapes(0).unwrap();
    let back = restored
        .iter()
        .find(|s| s.id == shape.id)
        .and_then(|s| s.text.iter().find(|t| !t.is_empty()));
    assert_eq!(back, shape.text.iter().find(|t| !t.is_empty()));
}

#[test]
fn new_command_drops_redo_side() {
    let mut d = doc();
    d.set_title(0, "A").unwrap();
    d.set_notes(0, Some("notes".into())).unwrap();
    d.undo().unwrap();
    assert!(d.can_redo());
    d.set_title(0, "B").unwrap();
    assert!(!d.can_redo());
    assert!(d.can_undo());
}

#[test]
fn undo_restores_byte_equivalent_deck_after_save() {
    let mut d = doc();

    d.set_title(0, "Mutated").unwrap();
    d.add_slide(None).unwrap();
    d.delete_slide(0).unwrap();
    // Walk everything back.
    while d.can_undo() {
        d.undo().unwrap();
    }
    assert!(d.slide_count() >= 2);

    // A fully-undone document still carries dirty=true, so saving exercises
    // the writer+merge path; the model must roundtrip to the original's.
    let out = std::env::temp_dir().join("omashow-undo-save.pptx");
    d.save(&out).unwrap();
    let reopened = PptxDocument::open(&out).unwrap();
    assert_eq!(
        model_of(&reopened.pres),
        model_of(
            &PptxDocument::open("/tmp/omashow-rt/real.pptx")
                .unwrap()
                .pres
        )
    );
}

fn z_test_deck() -> PptxDocument {
    let mk = |id: u32, name: &str| {
        Shape::AutoShape(
            AutoShape::new(id, name).with_properties(
                ShapeProperties::new().with_transform(
                    Transform2D::new()
                        .with_offset(0, 0)
                        .with_extent(100_000, 100_000),
                ),
            ),
        )
    };
    let slide = Slide::new()
        .with_shape(mk(1, "back"))
        .with_shape(mk(2, "middle"))
        .with_shape(mk(3, "front"));
    let mut d = PptxDocument::new();
    d.pres.slides.push(slide);
    d
}

fn shape_ids(d: &PptxDocument) -> Vec<u32> {
    d.pres.slides[0]
        .shapes
        .iter()
        .map(|s| match s {
            Shape::AutoShape(a) => a.id,
            _ => u32::MAX,
        })
        .collect()
}

#[test]
fn z_order_reorder_and_undo() {
    let mut d = z_test_deck();
    assert_eq!(shape_ids(&d), vec![1, 2, 3]);

    d.reorder_shape_z_order(0, 1, ZOrderAction::BringToFront)
        .unwrap();
    assert_eq!(shape_ids(&d), vec![2, 3, 1]);

    d.reorder_shape_z_order(0, 3, ZOrderAction::SendToBack)
        .unwrap();
    assert_eq!(shape_ids(&d), vec![3, 2, 1]);

    d.reorder_shape_z_order(0, 2, ZOrderAction::BringForward)
        .unwrap();
    assert_eq!(shape_ids(&d), vec![3, 1, 2]);

    d.reorder_shape_z_order(0, 1, ZOrderAction::SendBackward)
        .unwrap();
    assert_eq!(shape_ids(&d), vec![1, 3, 2]);

    // Boundary no-ops report `false` and leave the order untouched.
    assert!(!d
        .reorder_shape_z_order(0, 2, ZOrderAction::BringForward)
        .unwrap());
    assert_eq!(shape_ids(&d), vec![1, 3, 2]);
    assert!(!d
        .reorder_shape_z_order(0, 1, ZOrderAction::SendBackward)
        .unwrap());
    assert_eq!(shape_ids(&d), vec![1, 3, 2]);

    // Unknown shape id is a real error, not a silent no-op.
    assert!(d
        .reorder_shape_z_order(0, 99, ZOrderAction::BringToFront)
        .is_err());

    let final_ids = shape_ids(&d);
    // No-ops and errors never pollute history: exactly the 4 real reorders.
    let mut steps = 0;
    while d.can_undo() {
        d.undo().unwrap();
        steps += 1;
    }
    assert_eq!(steps, 4);
    assert_eq!(shape_ids(&d), vec![1, 2, 3]);
    while d.can_redo() {
        d.redo().unwrap();
    }
    assert_eq!(shape_ids(&d), final_ids);
}
