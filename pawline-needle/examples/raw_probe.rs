//! Prints the raw model reply for each probe given on the command line.
use pawline_needle::assist::NeedleAssist;
fn main() {
    let a = NeedleAssist::embedded().expect("load model");
    for p in std::env::args().skip(1) {
        a.needle().reset();
        let raw = a.needle().propose(&p, 128).expect("inference");
        println!("PROMPT {p}\nRAW {raw}\n");
    }
}
