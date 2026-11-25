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

    // libc++, libc++abi and LLVM libunwind (Apache-2.0 with LLVM exception).
    // Release builds link them STATICALLY (CACTUS_CPP_STATIC_DIR holds the
    // .a archives), so the shipped binary needs no libc++/libunwind at runtime.
    // Without it, fall back to dynamic linking for local development.
    if let Ok(dir) = std::env::var("CACTUS_CPP_STATIC_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=static=c++");
        println!("cargo:rustc-link-lib=static=c++abi");
        println!("cargo:rustc-link-lib=static=unwind");
        println!("cargo:rerun-if-env-changed=CACTUS_CPP_STATIC_DIR");
    } else {
        let cpp = std::env::var("CACTUS_CPP_LIB_DIR").unwrap_or_else(|_| "/tmp/cpplinks".into());
        println!("cargo:rustc-link-search=native={cpp}");
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=dylib=c++abi");
        println!("cargo:rustc-link-lib=dylib=unwind");
    }
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rustc-link-lib=dylib=dl");
}
