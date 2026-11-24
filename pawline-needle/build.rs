fn main() {
    // libc++ compatibility shim (legacy __hash_memory symbol) — our source,
    // compiled here so no system libc++ needs to export it.
    cc::Build::new().file("shim.c").compile("cactusshim");

    // Prebuilt Apache-2.0 Cactus runtime (static).
    println!(
        "cargo:rustc-link-search=native={}",
        std::env::var("CACTUS_LIB_DIR")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR")).to_string())
    );
    println!("cargo:rustc-link-lib=static=cactus");

    // libc++/libc++abi/libunwind live at CACTUS_CPP_LIB_DIR (no .so symlinks
    // in the extracted debs, hence the `:file.so.1` spelling).
    // directory of proper .so symlinks into the extracted libc++
    let cpp = std::env::var("CACTUS_CPP_LIB_DIR").unwrap_or_else(|_| "/tmp/cpplinks".into());
    println!("cargo:rustc-link-search=native={cpp}");
    println!("cargo:rustc-link-lib=dylib=c++");
    println!("cargo:rustc-link-lib=dylib=c++abi");
    println!("cargo:rustc-link-lib=dylib=unwind");
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rustc-link-lib=dylib=dl");
}
