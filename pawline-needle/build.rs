fn main() {

    // Build-time integrity check (item 16/20): the committed weights must be byte-identical to the
    // audited upstream artifact (docs/LICENSE-AUDIT.md, 2026-11-26). A swapped or corrupted
    // needle3.cact fails the build loudly instead of shipping a wrong model.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|t| t != "windows") {
        use sha2::Digest;
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/needle3.cact")).expect("needle3.cact missing");
        let got = format!("{:x}", sha2::Sha256::digest(&bytes));
        let want = "c9d915eca282ed42d1a09b143b592adb4cc6744ffe2d294adf5cfc5548170c38";
        assert!(got == want, "needle3.cact sha256 mismatch: got {got}, want {want} (see docs/LICENSE-AUDIT.md)");
        println!("cargo:rerun-if-changed=needle3.cact");
    }
    // The Cactus runtime we ship is a Linux static library. Windows builds have no embedded assist: lib.rs provides
    // stubs that report it as unavailable, so nothing is linked here.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|t| t == "windows") {
        return;
    }
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
