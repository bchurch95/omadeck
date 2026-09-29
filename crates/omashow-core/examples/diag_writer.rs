//! Is the office-toolkit WRITER itself lossless, or is PptxDocument's merge doing the work?
//! This bypasses PptxDocument and round-trips through the raw writer.
use office_toolkit::OpenFile;
use office_toolkit::SaveToFile;
use omashow_core::Presentation;

fn main() {
    let src = "/tmp/omashow-rt/real.pptx";
    let out = "/tmp/omashow-rt/pure_writer.pptx";

    // Pure writer round-trip: open model, edit title, save. NO merge.
    let mut pres: Presentation = Presentation::open_file(src).unwrap();
    // edit slide 0 title via the public API
    omashow_core::set_slide_title(&mut pres, 0, "Pure Writer Test").unwrap();
    pres.save_to_file(out).unwrap();

    let b = std::fs::read(src).unwrap();
    let before: Vec<String> = {
        use opc_ooxml::Package;
        use std::io::Cursor;
        let p = Package::read_from(Cursor::new(b)).unwrap();
        let mut v: Vec<String> = p
            .parts()
            .map(|x| x.name.trim_start_matches('/').to_string())
            .collect();
        v.sort();
        v
    };
    let after: Vec<String> = {
        use opc_ooxml::Package;
        use std::io::Cursor;
        let bb = std::fs::read(out).unwrap();
        let p = Package::read_from(Cursor::new(bb)).unwrap();
        let mut v: Vec<String> = p
            .parts()
            .map(|x| x.name.trim_start_matches('/').to_string())
            .collect();
        v.sort();
        v
    };
    let before_set: std::collections::HashSet<_> = before.iter().cloned().collect();
    let lost: Vec<&String> = before.iter().filter(|n| !after.contains(n)).collect();
    println!(
        "PURE WRITER (no merge): {} parts before, {} after, {} LOST",
        before.len(),
        after.len(),
        lost.len()
    );
    for n in &lost {
        println!("  lost: {}", n)
    }
    let _ = before_set;
}
