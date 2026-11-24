//! Proof: a Rust binary that runs Needle3 inference with the weights compiled
//! in. There is no model file argument, no extraction, no path — the bytes
//! come from the executable itself.

fn main() {
    let needle = pawline_needle::Needle::embedded(
        "You are Pawline's command assist.",
        r#"[{"name":"create_folder","description":"Create a folder","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]"#,
    )
    .expect("load embedded weights");
    let out = needle
        .propose("make a folder called kitten paws", 64)
        .expect("inference");
    println!("PROPOSAL: {out}");
    println!(
        "embedded bytes: {} (binary carries the model; no file used)",
        pawline_needle::NEEDLE3_CACT.len()
    );
}
