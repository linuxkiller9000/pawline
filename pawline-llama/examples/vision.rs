// Ask the in-process engine about an image, and print the numbers.
//   cargo run -p pawline-llama --features vendored --example vision -- model.gguf mmproj.gguf image.png "question"
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let model = args.next().expect("model path");
    let mmproj = args.next().expect("mmproj path");
    let image_path = args.next().expect("image path");
    let question = args
        .next()
        .unwrap_or_else(|| "What is on this screen? One short sentence.".into());
    let threads: u16 = std::env::var("PAWLINE_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);

    let image = match std::fs::read(&image_path) {
        Ok(b) => b,
        Err(e) => {
            println!("could not read {image_path}: {e}");
            std::process::exit(1);
        }
    };
    println!("image bytes: {}", image.len());
    println!("engine: {}", pawline_llama::engine_version());

    let t0 = Instant::now();
    let mut m = match pawline_llama::LocalModel::load(&model, 4096, threads) {
        Ok(m) => m,
        Err(e) => {
            println!("load failed: {e}");
            std::process::exit(1);
        }
    };
    println!("load: {:.2} s", t0.elapsed().as_secs_f64());

    let t1 = Instant::now();
    match m.load_vision(&mmproj) {
        Ok(()) => println!("projector: {:.2} s", t1.elapsed().as_secs_f64()),
        Err(e) => {
            println!("projector failed: {e}");
            std::process::exit(1);
        }
    }

    let t2 = Instant::now();
    match m.ask_image(&image, &question, 48) {
        Ok(reply) => println!("first ({:.2} s): {reply}", t2.elapsed().as_secs_f64()),
        Err(e) => println!("first failed: {e}"),
    }

    let t3 = Instant::now();
    match m.ask_image(&image, "Name the most prominent thing you see.", 32) {
        Ok(reply) => println!("second ({:.2} s): {reply}", t3.elapsed().as_secs_f64()),
        Err(e) => println!("second failed: {e}"),
    }
}
