// Ask the in-process engine one question and print the numbers.
//   cargo run -p pawline-llama --features vendored --example answer -- model.gguf "question"
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let model = args.next().expect("model path");
    let question = args.next().unwrap_or_else(|| "Say hello in five words.".into());
    let threads: u16 = std::env::var("PAWLINE_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);

    println!("engine: {}", pawline_llama::engine_version());
    println!("available: {}", pawline_llama::available());

    let t0 = Instant::now();
    let mut m = match pawline_llama::LocalModel::load(&model, 2048, threads) {
        Ok(m) => m,
        Err(e) => {
            println!("load failed: {e}");
            std::process::exit(1);
        }
    };
    println!("load: {:.2} s", t0.elapsed().as_secs_f64());

    let system = "You are Pawline, a small local assistant. Answer in one short sentence.";
    let t1 = Instant::now();
    match m.answer(system, &question, 48) {
        Ok(reply) => {
            let dt = t1.elapsed().as_secs_f64();
            println!("reply ({dt:.2} s): {reply}");
        }
        Err(e) => println!("generation failed: {e}"),
    }

    // Second question on the same loaded model: no reload, no process start.
    let t2 = Instant::now();
    match m.answer(system, "Name one colour.", 24) {
        Ok(reply) => println!("second ({:.2} s): {reply}", t2.elapsed().as_secs_f64()),
        Err(e) => println!("second failed: {e}"),
    }
}
