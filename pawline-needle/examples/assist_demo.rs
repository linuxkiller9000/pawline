//! Live loop: an input the deterministic parser refuses -> Needle3 proposes
//! -> core validates -> DryRun shows what would happen. Real inference, no
//! model file.

use pawline_core::exec::DryRunBackend;
use pawline_core::{Engine, Response};
use pawline_needle::assist::NeedleAssist;

fn main() {
    let root = std::env::temp_dir().join("pawline-assist-demo");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let assist = NeedleAssist::embedded().expect("load embedded Needle3");
    let mut engine = Engine::new(root.clone());
    let backend = DryRunBackend { log: Default::default() };

    // First: what does the model actually propose for these?
    for probe in ["conjure up a directory named starlight for me", "please whip up a file called ideas.md"] {
        let raw = assist.needle().propose(probe, 128).expect("inference");
        println!("RAW {probe}: {raw}");
    }
    for input in [
        "conjure up a directory named starlight for me",
        "please whip up a file called ideas.md",
    ] {
        let r = match engine.handle_with(input, &backend, false, Some(&assist)) {
            Ok(r) => r,
            Err(e) => {
                println!("\"{input}\" -> refused: {e}");
                continue;
            }
        };
        match r {
            Response::Executed(ops) => println!("\"{input}\" -> {:?}", ops),
            Response::Confirm(p) => {
                println!("\"{input}\" -> SHOWN TO USER: {}", p.summary().trim());
                let _r2 = engine.execute_plan(p, &backend).expect("execute confirmed");
                println!("   user said yes -> executed");
            }
            _other => println!("\"{input}\" -> other"),
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}
