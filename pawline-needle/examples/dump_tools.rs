/// Writes Pawline's tool list to /tmp/tools.json for the bench examples.
fn main() {
    std::fs::write("/tmp/tools.json", pawline_needle::assist::PAWLINE_TOOLS).unwrap();
    println!("wrote /tmp/tools.json ({} bytes)", pawline_needle::assist::PAWLINE_TOOLS.len());
}
