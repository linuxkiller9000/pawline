fn main() {
    pkg_config::Config::new().probe("gtk-layer-shell-0").unwrap();
    println!("cargo:rustc-link-lib=dylib=gtk-3");
}
