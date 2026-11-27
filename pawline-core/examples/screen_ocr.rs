//! Live check: preprocess a real screenshot with screencap::prepare_for_ocr
//! and print the PGM to a file for tesseract. Usage: screen_ocr <in.png> <out.pgm>
fn main() {
    let png = std::fs::read(std::env::args().nth(1).expect("input png")).unwrap();
    let pgm = pawline_core::screencap::prepare_for_ocr(&png).unwrap();
    std::fs::write(std::env::args().nth(2).expect("output pgm"), &pgm).unwrap();
    println!("pgm bytes: {}", pgm.len());
}
