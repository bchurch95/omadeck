use opc_ooxml::Package;
use std::io::Cursor;

fn main() {
    let src = "/tmp/omashow-rt/real.pptx";
    let b = std::fs::read(src).unwrap();
    let pkg = Package::read_from(Cursor::new(b)).unwrap();
    let mut v: Vec<String> = pkg.parts().map(|x| x.name.clone()).collect();
    v.sort();
    for n in v {
        println!("{}", n)
    }
}
