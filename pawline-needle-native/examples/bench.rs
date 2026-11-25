use needle_infer::v3_engine::{KvPrecision, V3Engine, V3Options};
use std::time::Instant;
fn main() {
    let tools = std::fs::read_to_string("/tmp/tools.json").unwrap();
    let q = "rename report.txt to final.txt";
    for (label, depth, kv) in [("d20 f32", 20usize, KvPrecision::F32), ("d20 int8", 20, KvPrecision::Int8), ("d12 f32", 12, KvPrecision::F32)] {
        let e = V3Engine::from_bytes_with_depth(pawline_needle_native::NEEDLE3_CACT.to_vec(), depth).unwrap();
        let o = V3Options { max_new_tokens: 128, constrain: true, kv_precision: kv, ..V3Options::default() };
        let t = Instant::now();
        let r = e.generate(q, &tools, &o);
        let g = t.elapsed();
        let t = Instant::now();
        let c = e.confidence_for(q, &tools, &r.text);
        println!("{label}: generate {:?} (positions {}, new tokens {}) confidence {:?} -> {:?} | text {}", g, r.positions, r.tokens.len(), t.elapsed(), c, r.text.chars().take(90).collect::<String>().replace('\n', " "));
    }
}
