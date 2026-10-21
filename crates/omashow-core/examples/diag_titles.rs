//! Throwaway: dump slide titles + shape counts for heuristic calibration.
use omashow_core::{model_of, PptxDocument};

fn main() {
    let path = std::env::args().nth(1).expect("usage: diag_titles <deck>");
    let pres = PptxDocument::open(&path).expect("read deck");
    let model = model_of(&pres.pres);
    for s in &model.slides {
        let shapes = pres
            .pres
            .slides
            .get(s.index)
            .map(|sl| sl.shapes.len())
            .unwrap_or(0);
        let title = s.title.as_deref().unwrap_or("<none>");
        let notes = s.notes.is_some();
        println!(
            "[{:02}] shapes={:02} notes={} title={:?}",
            s.index, shapes, notes, title
        );
    }
}
