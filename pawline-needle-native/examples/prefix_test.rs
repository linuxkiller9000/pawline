use needle_core::v3::V3Cache;
use needle_infer::prompt::build_prompt;
use needle_infer::v3_engine::V3Engine;
use std::time::Instant;
fn main() {
    let tools = std::fs::read_to_string("/tmp/tools.json").unwrap();
    let e = V3Engine::from_bytes(pawline_needle_native::NEEDLE3_CACT.to_vec()).unwrap();
    let bos = e.tokenizer.id_of("<s>").unwrap_or(2);
    let q = "rename report.txt to final.txt";
    let full = build_prompt(q, &tools, None);
    let marker = "</tools>\n";
    let cut = full.find(marker).unwrap() + marker.len();
    let (pre, rest) = full.split_at(cut);
    let mut full_ids = vec![bos]; full_ids.extend(e.tokenizer.encode(&full));
    let mut pre_ids = vec![bos]; pre_ids.extend(e.tokenizer.encode(pre));
    println!("full {} tokens, prefix {} tokens, prefix_match={}", full_ids.len(), pre_ids.len(), full_ids[..pre_ids.len()] == pre_ids[..]);
    let n = full_ids.len() + 64;
    let t = Instant::now();
    let mut c1 = V3Cache::new(&e.model.cfg, n);
    let l1 = e.model.prefill(&full_ids, &mut c1);
    println!("full prefill {:?}", t.elapsed());
    let t = Instant::now();
    let mut base = V3Cache::new(&e.model.cfg, n);
    let _ = e.model.prefill(&pre_ids, &mut base);
    println!("prefix prefill (one time) {:?}", t.elapsed());
    let t = Instant::now();
    let mut c2 = base.clone();
    let mut l2 = Vec::new(); for &t in &full_ids[pre_ids.len()..] { l2 = e.model.decode_step(&mut c2, t); }
    println!("suffix stepped from cloned prefix cache {:?}", t.elapsed());
    let bits_equal = l1.iter().zip(&l2).all(|(a, b)| a.to_bits() == b.to_bits());
    let maxd = l1.iter().zip(&l2).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    println!("logits bit-identical={bits_equal} max_abs_diff={maxd} pos {} vs {}", c1.pos(), c2.pos());
    let _ = rest;
    // continue decoding a few greedy steps from both caches and compare ids
    let (mut a, mut b) = (l1, l2);
    let mut same = true;
    for _ in 0..12 {
        let ta = a.iter().cloned().enumerate().fold((0, f32::MIN), |m, (i, v)| if v > m.1 { (i, v) } else { m }).0 as u32;
        let tb = b.iter().cloned().enumerate().fold((0, f32::MIN), |m, (i, v)| if v > m.1 { (i, v) } else { m }).0 as u32;
        if ta != tb { same = false; }
        a = e.model.decode_step(&mut c1, ta); b = e.model.decode_step(&mut c2, tb);
    }
    println!("12 greedy continuation ids identical={same}");
}
