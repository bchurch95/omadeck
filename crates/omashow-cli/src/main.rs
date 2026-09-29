use clap::{Parser, Subcommand};
use omashow_core::{
    model_of, open_pptx, save_pptx, PptxDocument, PresentationModel, SlideDimensions,
};
use serde::Serialize;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    New {
        output: String,
    },
    Open {
        input: String,
    },
    Save {
        input: String,
        output: String,
    },
    List {
        input: String,
    },
    Export {
        input: String,
        output: String,
    },
    ExportPdf {
        input: String,
        output: String,
    },
    ExportHtml {
        input: String,
        output: String,
    },
    Edit {
        input: String,
        output: String,
        slide: usize,
        title: String,
    },
    Inspect {
        input: String,
    },
}

/// One slide of the `inspect` output: metadata plus the full shape view.
#[derive(Serialize)]
struct SlideInspect {
    index: usize,
    title: Option<String>,
    notes: Option<String>,
    shapes: Vec<omashow_core::ShapeInfo>,
}

/// Structured deck description for `inspect`.
#[derive(Serialize)]
struct InspectOutput {
    title: String,
    slide_count: usize,
    slide_dimensions: SlideDimensions,
    slides: Vec<SlideInspect>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::New { output } => {
            let model = PresentationModel {
                title: "Untitled".into(),
                slides: vec![],
            };
            save_pptx(&output, &model)?;
            println!("Created {}", output);
        }
        Commands::Open { input } => {
            let model = open_pptx(&input)?;
            println!("Opened {} with {} slides", input, model.slides.len());
        }
        Commands::Save { input, output } => {
            let model = open_pptx(&input)?;
            save_pptx(&output, &model)?;
            println!("Saved {} -> {}", input, output);
        }
        Commands::List { input } => {
            let model = open_pptx(&input)?;
            println!("Presentation: {}", model.title);
            for s in model.slides {
                println!("  Slide {}: {:?}", s.index + 1, s.title);
            }
        }
        Commands::Export { input, output } => {
            let model = open_pptx(&input)?;
            std::fs::write(&output, serde_json::to_string_pretty(&model)?)?;
            println!("Exported {} -> {}", input, output);
        }
        Commands::ExportPdf { input, output } => {
            let doc = PptxDocument::open(&input)?;
            doc.export_pdf(&output)?;
            println!("Exported {} slides -> {}", doc.slide_count(), output);
        }
        Commands::ExportHtml { input, output } => {
            let doc = PptxDocument::open(&input)?;
            doc.export_html(&output)?;
            println!("Exported {} slides -> {}", doc.slide_count(), output);
        }
        Commands::Edit {
            input,
            output,
            slide,
            title,
        } => {
            let mut model = open_pptx(&input)?;
            if let Some(s) = model.slides.get_mut(slide) {
                s.title = Some(title);
                save_pptx(&output, &model)?;
                println!("Edited slide {} -> {}", slide + 1, output);
            } else {
                anyhow::bail!("Slide {} not found", slide + 1);
            }
        }
        Commands::Inspect { input } => {
            let doc = PptxDocument::open(&input)?;
            let model = model_of(&doc.pres);
            let mut slides = Vec::with_capacity(doc.slide_count());
            for i in 0..doc.slide_count() {
                let shapes = doc.get_slide_shapes(i)?;
                slides.push(SlideInspect {
                    index: i,
                    title: model.slides[i].title.clone(),
                    notes: model.slides[i].notes.clone(),
                    shapes,
                });
            }
            let out = InspectOutput {
                title: model.title,
                slide_count: doc.slide_count(),
                slide_dimensions: doc.slide_dimensions(),
                slides,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
    }
    Ok(())
}
