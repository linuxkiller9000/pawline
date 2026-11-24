fn main() {
    for s in ["inside banana create a folder called x",
              "Inside pics create a Markdown file called kitten paws.md",
              "Inside it create folders called gumpus, katkota, and pics"] {
        println!("INPUT: {s}");
        match pawline_core::intent::parse(s) {
            Ok(d) => println!("  OK: {d:?}"),
            Err(e) => println!("  ERR: {e}"),
        }
    }
}
