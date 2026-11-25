//! TEST-ONLY probe, not part of the app: loads the four pinned libs from bytes compiled into
//! this executable (same Runtime::load path the embed-voice app uses) and transcribes a clip.
//! Build: PAWLINE_VOICE_EMBED_DIR=<dir with the 4 libs> cargo build --release -p pawline-voice --example embed_probe
//! Run:   embed_probe <model dir> <f32le 16 kHz pcm file>
use pawline_voice::runtime::{PinnedLib, Runtime};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let model = &a[1];
    let pcm = std::fs::read(&a[2]).unwrap();
    let samples: Vec<f32> = pcm.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let b: [&[u8]; 4] = [
        include_bytes!(concat!(env!("PAWLINE_VOICE_EMBED_DIR"), "/libgcc_s.so.1")),
        include_bytes!(concat!(env!("PAWLINE_VOICE_EMBED_DIR"), "/libstdc++.so.6")),
        include_bytes!(concat!(env!("PAWLINE_VOICE_EMBED_DIR"), "/libonnxruntime.so.1")),
        include_bytes!(concat!(env!("PAWLINE_VOICE_EMBED_DIR"), "/libmoonshine.so")),
    ];
    let pins = [
        ("libgcc_s.so.1", "fc9d43b2f6c20e53b009238f767c5b949d202389e20de9e202ea684b4ba3729a"),
        ("libstdc++.so.6", "ff0825e113603c3866680d5d52216bc6d8eedf3a59f52a0aef67ff01994db128"),
        ("libonnxruntime.so.1", "13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b"),
        ("libmoonshine.so", "85be190fe5f8764c65d81f5c99c33a68327c5570534e35b62a4fafa5cdaa6dc1"),
    ];
    let libs: Vec<PinnedLib> = pins.iter().zip(b.iter()).map(|((n, h), by)| PinnedLib { name: n, bytes: by, sha256_hex: h }).collect();
    let rt = Runtime::load(&libs).expect("load");
    println!("loaded 4 embedded libs from memory");
    for i in 1..=2 {
        let t0 = std::time::Instant::now();
        println!("call {i}: {:?} ({:.1}s)", rt.transcribe(model, &samples).expect("transcribe"), t0.elapsed().as_secs_f32());
    }
}
