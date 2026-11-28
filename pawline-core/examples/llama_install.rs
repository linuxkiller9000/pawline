//! Manual check of the llama-server install path, end to end: fetch the pinned
//! asset, verify size and sha-256, extract with tar, prove the binary starts.
//!
//!   cargo run -p pawline-core --example llama_install -- /tmp/some-home
//!
//! It downloads about 22 MB, so it is a deliberate run, never part of `cargo test`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn main() {
    let home = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pawline-llama-install"));
    let Some(c) = pawline_core::llamainstall::candidate(std::env::consts::OS, std::env::consts::ARCH) else {
        println!("no pinned build for {} {}", std::env::consts::OS, std::env::consts::ARCH);
        std::process::exit(1);
    };
    println!("{}", pawline_core::llamainstall::card_text(&c, &home));
    println!("---");
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    match pawline_core::llamainstall::install_live(&c, &home, &cancel, &|line| println!("{line}")) {
        Ok(bin) => {
            println!("installed: {}", bin.display());
            println!("took {:.1} s", started.elapsed().as_secs_f64());
            println!("check: {:?}", pawline_core::llamainstall::check_starts(&bin));
        }
        Err(e) => {
            println!("failed: {e}");
            std::process::exit(1);
        }
    }
}
