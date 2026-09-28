# CURRENT TASK: Slide Background Engine (`p:bg`) (Milestone 10)

## Goal
Support custom slide background fills (`<p:bg>`), extract them during slide inspection, render them on the slide canvas/filmstrip in frontend, and allow setting slide background with undo support.

## Exact Types & Architecture (DO NOT search .cargo/registry!)

1. `office_toolkit::powerpoint::Slide`:
```rust
pub struct Slide {
    pub shapes: Vec<Shape>,
    pub notes: Option<TextBody>,
    pub background: Option<Fill>, // Fill::Solid(Color), Fill::None, etc.
    // ...
}
```
`Slide::with_background(mut self, background: Fill) -> Self` already exists in `powerpoint-ooxml`!

2. `office_toolkit::drawing::Fill` & `Color`:
```rust
pub enum Fill {
    None,
    Solid(Color),
    Gradient(GradientFill),
    Pattern(PatternFill),
    Image(ImageFill),
}
```

## Step 1: Backend `crates/omashow-core/src/inspect.rs`
1. Add `slide_background(pres: &Presentation, slide: usize) -> Result<Option<String>, Error>`:
```rust
pub fn slide_background(pres: &Presentation, slide: usize) -> Result<Option<String>, Error> {
    let s = pres.slides.get(slide).ok_or(Error::OutOfRange(slide))?;
    Ok(s.background.as_ref().map(fill_to_css))
}
```
(Notice `fill_to_css` is already defined in `inspect.rs`!)

2. In `crates/omashow-core/src/lib.rs`:
Export `slide_background`:
```rust
pub use inspect::{
    get_slide_shapes, slide_background, slide_count, slide_dimensions, BoundingBox, LineInfo, ShapeInfo,
    SlideDimensions, TextRunInfo,
};
```
Add mutation helper:
```rust
pub fn set_slide_background(pres: &mut Presentation, slide: usize, fill: Option<Fill>) -> Result<(), Error> {
    let s = pres.slides.get_mut(slide).ok_or(Error::OutOfRange(slide))?;
    s.background = fill;
    Ok(())
}
```

3. In `crates/omashow-core/src/document.rs`:
Expose `slide_background(&self, slide: usize) -> Result<Option<String>, Error>`:
```rust
pub fn slide_background(&self, slide: usize) -> Result<Option<String>, Error> {
    crate::inspect::slide_background(&self.pres, slide)
}
```
Add undoable `set_slide_background`:
In `crates/omashow-core/src/undo.rs`:
Add `Background` variant to `UndoCommand`:
```rust
    Background {
        slide: usize,
        before: Option<Fill>,
        after: Option<Fill>,
        description: String,
    },
```
(Remember to update `apply` and `revert` match arms for `Background` in `undo.rs`!)
In `document.rs`:
```rust
pub fn set_slide_background(&mut self, slide: usize, fill: Option<Fill>) -> Result<(), Error> {
    let before = self.pres.slides.get(slide).map(|s| s.background.clone()).ok_or(Error::OutOfRange(slide))?;
    crate::set_slide_background(&mut self.pres, slide, fill.clone())?;
    self.history.record(UndoCommand::Background {
        slide,
        before,
        after: fill,
        description: "change slide background".into(),
    });
    self.dirty = true;
    Ok(())
}
```

## Step 2: Update SlideContent & Frontend
1. In `apps/omashow-tauri/src-tauri/src/main.rs`:
Update `SlideContent`:
```rust
#[derive(Serialize)]
struct SlideContent {
    index: usize,
    slide_dimensions: SlideDimensions,
    background: Option<String>,
    shapes: Vec<omashow_core::ShapeInfo>,
}
```
In `get_slide_content`:
```rust
    let bg = doc.slide_background(slide).map_err(|e| e.to_string())?;
    Ok(SlideContent {
        index: slide,
        slide_dimensions: doc.slide_dimensions(),
        background: bg,
        shapes,
    })
```
2. In `apps/omashow-tauri/src/frontend/slide_render.js`:
In `renderSlideInto(container, content, mini = false)`:
Apply slide background:
```javascript
  if (content.background && !NON_SOLID_FILLS.has(content.background)) {
    const bg = themeColor(content.background);
    container.style.backgroundColor = bg;
  } else {
    container.style.backgroundColor = "";
  }
```

## Step 3: Tests & Verification
In `crates/omashow-core/tests/inspect.rs`:
Add test `slide_background_extraction_and_roundtrip`:
```rust
#[test]
fn slide_background_extraction() {
    use office_toolkit::drawing::{Color, Fill};
    let mut pres = Presentation::new();
    let slide = Slide::new().with_background(Fill::Solid(Color::Rgb("336699".to_string())));
    pres.slides.push(slide);

    let bg = omashow_core::slide_background(&pres, 0).expect("background extraction");
    assert_eq!(bg, Some("#336699".to_string()));
}
```
Run `cargo test -p omashow-core` to verify all tests pass!

## Step 4: Complete & Move Forward
- Mark `[x] Slide Background Engine (`p:bg`)` in TODO.md.
- Commit: `feat(background): add slide background extraction, canvas rendering, and undoable mutation`
- Then immediately continue to the next task in TODO.md (`Z-Order Controls & Stacking`).
