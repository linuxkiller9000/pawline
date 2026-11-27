/// Converts the real Laya int8 model to fp32 using the in-app converter.
/// Usage: cargo run -p pawline-core --example laya_convert_run -- <in.onnx> <out.onnx> <out.data>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (inp, outp, outd) = (&args[1], &args[2], &args[3]);
    let bytes = std::fs::read(inp).unwrap();
    println!("input: {} bytes", bytes.len());
    let t0 = std::time::Instant::now();
    let mut data = std::io::BufWriter::new(std::fs::File::create(outd).unwrap());
    let mut last = 0;
    let (model, stats) = pawline_core::laya_convert::convert_onnx_bytes(&bytes, &mut data, &mut |n| {
        if n / 50 != last {
            last = n / 50;
            println!("  node {n} ({:?})", t0.elapsed());
        }
        Ok(())
    })
    .unwrap();
    drop(data);
    std::fs::write(outp, &model).unwrap();
    println!(
        "done in {:?}: {} nodes converted, {} initializers replaced, {} external bytes, graph {} bytes",
        t0.elapsed(), stats.nodes_converted, stats.initializers_replaced, stats.external_bytes, model.len()
    );
}
