//! Manual smoke: sets the wallpaper through the real system adapter.
use pawline_core::wallpaper::{SystemWallpaper, WallpaperSetter};
fn main() {
    let p = std::env::args().nth(1).expect("image path");
    match SystemWallpaper.set(std::path::Path::new(&p), std::env::args().nth(2).and_then(|n| pawline_core::wallpaper::Adapter::from_name(&n))) {
        Ok(m) => println!("ok: {m}"),
        Err(e) => {
            println!("err: {e}");
            std::process::exit(1)
        }
    }
}
