use office_toolkit::drawing::{ShapeProperties, TextBody, TextParagraph, TextRun, Transform2D};
use office_toolkit::powerpoint::{AutoShape, Shape, Slide};
use office_toolkit::prelude::*;
use office_toolkit::SaveToFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text_box = AutoShape::new(2, "Title")
        .with_properties(
            ShapeProperties::new().with_transform(
                Transform2D::new()
                    .with_offset(1_000_000, 1_000_000)
                    .with_extent(7_772_400, 1_200_150),
            ),
        )
        .with_text_body(
            TextBody::new()
                .with_paragraph(TextParagraph::new().with_run(TextRun::text("Hello from Omashow"))),
        );

    let slide = Slide::new().with_shape(Shape::AutoShape(text_box));
    let presentation = Presentation::new().with_slide(slide);
    presentation.save_to_file("demo.pptx")?;
    println!("Created demo.pptx");
    Ok(())
}
