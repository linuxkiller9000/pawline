fn main() {
    for k in [250usize, 500, 1000, 2000, 4000] {
        let s = "delete ".repeat(k);
        let t = std::time::Instant::now();
        let r = pawline_core::intent::parse(&s);
        println!("tokens={k} bytes={} -> {:?} in {:?}", s.len(), r.map(|v| v.len()).map_err(|e| e.to_string()), t.elapsed());
    }
    // also probe math path directly (out-empty inputs go there)
    for k in [1000usize, 4000, 16000] {
        let s = "zzzq ".repeat(k);
        let t = std::time::Instant::now();
        let r = pawline_core::intent::parse(&s);
        println!("zzzq tokens={k} -> err? {} in {:?}", r.is_err(), t.elapsed());
    }
}
