//! Manual smoke: sets the wallpaper through the real system adapter.
use pawline_core::wallpaper::{SystemWallpaper, WallpaperSetter};
fn main() {
    let p = std::env::args().nth(1).expect("image path");
    match SystemWallpaper.set(std::path::Path::new(&p)) {
        Ok(m) => println!("ok: {m}"),
        Err(e) => {
            println!("err: {e}");
            std::process::exit(1)
        }
    }
}
