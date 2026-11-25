use pawline_needle::assist::{ASSIST_SYSTEM_PROMPT, PAWLINE_TOOLS};
fn main() {
    let n = pawline_needle::Needle::embedded(ASSIST_SYSTEM_PROMPT, PAWLINE_TOOLS).expect("load");
    let p = std::fs::read_to_string("/tmp/prompts.txt").unwrap();
    for q in p.lines() {
        n.reset();
        println!("{q}\t{}", n.propose(q, 128).unwrap_or_else(|e| e.0).replace('\n', " "));
    }
}
