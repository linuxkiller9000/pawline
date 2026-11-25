fn main() {
    let tools = std::fs::read_to_string("/tmp/tools.json").unwrap();
    let n = pawline_needle_native::NeedleNative::embedded(&tools).expect("load");
    let p = std::fs::read_to_string("/tmp/prompts.txt").unwrap();
    for q in p.lines() { println!("{q}\t{}", n.propose(q, 128)); }
}
