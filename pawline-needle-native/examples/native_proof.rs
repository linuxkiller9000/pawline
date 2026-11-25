fn main() {
    let tools = std::env::var("TOOLS").unwrap_or_else(|_| {
        r#"[{"name":"create_folder","description":"Create a folder","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]"#.into()
    });
    let n = pawline_needle_native::NeedleNative::embedded(&tools).expect("load");
    for q in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        println!("{q} => {} ({:?})", n.propose(&q, 128), t.elapsed());
    }
    println!("embedded bytes: {}", pawline_needle_native::NEEDLE3_CACT.len());
}
