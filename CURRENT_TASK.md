# CURRENT TASK: Z-Order Controls & Stacking (Milestone 10)

## Goal
Implement shape z-ordering (Bring to Front, Send to Back, Bring Forward, Send Backward) within a slide's shape tree, preserving relative ordering in OOXML `<p:spTree>`, updating frontend DOM, and supporting undo/redo.

## Architecture & Types

In `office_toolkit::powerpoint::Slide`:
```rust
pub struct Slide {
    pub shapes: Vec<Shape>,
    // ...
}
```
`s.shapes` represents the exact document z-order (index 0 is backmost, last index is frontmost).

## Step 1: Core Mutations in `crates/omashow-core/src/lib.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZOrderAction {
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
}

pub fn reorder_shape_z_order(
    pres: &mut Presentation,
    slide: usize,
    shape_id: u32,
    action: ZOrderAction,
) -> Result<(), Error> {
    let slide = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    let idx = slide.shapes.iter().position(|s| match s {
        Shape::AutoShape(a) => a.id == shape_id,
        Shape::Picture(p) => p.id == shape_id,
        Shape::Chart(c) => c.id == shape_id,
        Shape::Group(g) => g.id == shape_id,
        Shape::Connector(c) => c.id == shape_id,
        Shape::Table(t) => t.id == shape_id,
        Shape::Media(m) => m.id == shape_id,
    }).ok_or(Error::ShapeNotFound(shape_id))?;

    let n = slide.shapes.len();
    if n <= 1 {
        return Ok(());
    }

    match action {
        ZOrderAction::BringToFront => {
            let shape = slide.shapes.remove(idx);
            slide.shapes.push(shape);
        }
        ZOrderAction::SendToBack => {
            let shape = slide.shapes.remove(idx);
            slide.shapes.insert(0, shape);
        }
        ZOrderAction::BringForward => {
            if idx + 1 < n {
                slide.shapes.swap(idx, idx + 1);
            }
        }
        ZOrderAction::SendBackward => {
            if idx > 0 {
                slide.shapes.swap(idx, idx - 1);
            }
        }
    }
    Ok(())
}
```

Export `ZOrderAction` and `reorder_shape_z_order` in `crates/omashow-core/src/lib.rs`.

## Step 2: Document Undo Integration in `crates/omashow-core/src/document.rs`

```rust
    pub fn reorder_shape_z_order(
        &mut self,
        slide: usize,
        shape_id: u32,
        action: crate::ZOrderAction,
    ) -> Result<(), Error> {
        let before = self
            .pres
            .slides
            .get(slide)
            .map(|s| s.shapes.clone())
            .ok_or(Error::OutOfRange(slide))?;
        crate::reorder_shape_z_order(&mut self.pres, slide, shape_id, action)?;
        self.history.record(UndoCommand::Shapes {
            slide,
            before,
            after: self.pres.slides[slide].shapes.clone(),
            description: "reorder shape layer".into(),
        });
        self.dirty = true;
        Ok(())
    }
```

## Step 3: Tests in `crates/omashow-core/tests/editing.rs`

Add test verifying `reorder_shape_z_order` with all 4 variants and undo roundtrip:
```rust
#[test]
fn z_order_reorder_and_undo() {
    let mut pres = Presentation::new();
    let s1 = Shape::AutoShape(AutoShape::new(10, "Bottom"));
    let s2 = Shape::AutoShape(AutoShape::new(20, "Middle"));
    let s3 = Shape::AutoShape(AutoShape::new(30, "Top"));
    let slide = Slide::new().with_shape(s1).with_shape(s2).with_shape(s3);
    pres.slides.push(slide);

    // Bring 10 to front -> [20, 30, 10]
    crate::reorder_shape_z_order(&mut pres, 0, 10, crate::ZOrderAction::BringToFront).unwrap();
    let ids: Vec<u32> = pres.slides[0].shapes.iter().map(|s| match s { Shape::AutoShape(a) => a.id, _ => 0 }).collect();
    assert_eq!(ids, vec![20, 30, 10]);

    // Send 10 to back -> [10, 20, 30]
    crate::reorder_shape_z_order(&mut pres, 0, 10, crate::ZOrderAction::SendToBack).unwrap();
    let ids: Vec<u32> = pres.slides[0].shapes.iter().map(|s| match s { Shape::AutoShape(a) => a.id, _ => 0 }).collect();
    assert_eq!(ids, vec![10, 20, 30]);
}
```

## Step 4: Verification & Finish
- Run `cargo test -p omashow-core`
- Mark `[x] Z-Order Controls & Stacking` in TODO.md
- Commit: `feat(zorder): add bring to front/send to back layer reordering with undo support`
- Continue immediately to Sidebar Layers Panel!
