use needle_infer::v3_engine::{KvPrecision, V3Engine, V3Options};
use std::time::Instant;
fn main() {
    let tools = std::fs::read_to_string("/tmp/tools.json").unwrap();
    let q = "rename report.txt to final.txt";
    // Optional filters so a slow or memory-tight machine can run one slice per
    // invocation: argv[1] = config label prefix ("d20", "d12", exact "d20 f32"),
    // BENCH_MAX_NEW = cap on generated tokens (default 128).
    let filter = std::env::args().nth(1);
    let max_new: usize = std::env::var("BENCH_MAX_NEW").ok().and_then(|v| v.parse().ok()).unwrap_or(128);
    for (label, depth, kv) in [("d20 f32", 20usize, KvPrecision::F32), ("d20 int8", 20, KvPrecision::Int8), ("d12 f32", 12, KvPrecision::F32)] {
        if let Some(f) = &filter {
            if !label.starts_with(f.as_str()) {
                continue;
            }
        }
        let e = V3Engine::from_bytes_with_depth(pawline_needle_native::NEEDLE3_CACT.to_vec(), depth).unwrap();
        let o = V3Options { max_new_tokens: max_new, constrain: true, kv_precision: kv, ..V3Options::default() };
        let t = Instant::now();
        let r = e.generate(q, &tools, &o);
        let g = t.elapsed();
        let t = Instant::now();
        let c = e.confidence_for(q, &tools, &r.text);
        println!("{label}: generate {:?} (positions {}, new tokens {}, {:.2} tok/s) confidence {:?} -> {:?} | text {}", g, r.positions, r.tokens.len(), r.tokens.len() as f64 / g.as_secs_f64(), t.elapsed(), c, r.text.chars().take(90).collect::<String>().replace('\n', " "));
    }
}
