// Build the vendored llama.cpp source into static libraries and link them with
// a small C shim, so Rust only ever sees plain scalars and char buffers.
//
// Source location, in order: $PAWLINE_LLAMA_SRC, then <repo>/vendor/llama.cpp.
// Feature "vendored" off => the stub shim is compiled and the engine reports
// itself unavailable, so the workspace still builds with no C++ toolchain.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PAWLINE_LLAMA_SRC");
    println!("cargo:rerun-if-env-changed=PAWLINE_LLAMA_BUILD_DIR");

    let vendored = env::var_os("CARGO_FEATURE_VENDORED").is_some();
    if !vendored {
        let mut b = cc::Build::new();
        b.file("src/shim_stub.c").warnings(false);
        b.compile("pawline_shim");
        println!("cargo:rustc-cfg=pawline_llama_stub");
        return;
    }

    let src = source_dir();
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // Reuse an existing cmake build tree when one is pointed at us: the build
    // is minutes long on a small box and CI/dev runs should not repeat it.
    let build = match env::var_os("PAWLINE_LLAMA_BUILD_DIR") {
        Some(p) => PathBuf::from(p),
        None => out_dir.join("llama-build"),
    };
    let profile = env::var("PROFILE").unwrap_or_else(|_| "release".into());
    let build_type = if profile == "release" { "Release" } else { "Debug" };

    let reuse = find_lib_dirs(&build).iter().any(|d| d.join("libllama.a").is_file());
    if !reuse {
    let cfg = Command::new("cmake")
        .arg("-S").arg(&src)
        .arg("-B").arg(&build)
        .arg(format!("-DCMAKE_BUILD_TYPE={build_type}"))
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DLLAMA_BUILD_TESTS=OFF")
        .arg("-DLLAMA_BUILD_EXAMPLES=OFF")
        .arg("-DLLAMA_BUILD_SERVER=OFF")
        .arg("-DLLAMA_CURL=OFF")
        .arg("-DLLAMA_OPENSSL=OFF")
        .arg("-DGGML_NATIVE=OFF")
        .arg("-DGGML_CPU_ALL_VARIANTS=ON")
        .arg("-DGGML_OPENMP=OFF")
        .status()
        .expect("cmake configure failed to start");
    assert!(cfg.success(), "cmake configure of vendored llama.cpp failed");

    let built = Command::new("cmake")
        .arg("--build").arg(&build)
        .arg("--target").arg("llama")
        .status()
        .expect("cmake build failed to start");
    assert!(built.success(), "cmake build of vendored llama.cpp failed");
    }

    let mut shim = cc::Build::new();
    shim.file("src/shim.c")
        .include(src.join("include"))
        .include(src.join("ggml/include"))
        .include(src.join("tools/mtmd"))
        .warnings(false)
        .opt_level(2);
    shim.compile("pawline_shim");

    for d in find_lib_dirs(&build) {
        println!("cargo:rustc-link-search=native={}", d.display());
    }
    // mtmd links these privately, so a static archive does not carry them: the
    // sha-256 helper it uses for image ids lives here.
    let vendor_hash = build.join("vendor/hash");
    if vendor_hash.is_dir() {
        println!("cargo:rustc-link-search=native={}", vendor_hash.display());
        println!("cargo:rustc-link-lib=static=vendor-hash");
    }
    for lib in ["mtmd", "llama", "ggml-cpu", "ggml", "ggml-base"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=dylib=stdc++");
    // A reused cmake tree may have been configured with OpenMP on (ggml's
    // default); linking gomp keeps that tree usable. Our own builds set
    // -DGGML_OPENMP=OFF, where this is a harmless no-op.
    println!("cargo:rustc-link-lib=dylib=gomp");
    println!("cargo:rustc-link-lib=dylib=m");
}

fn source_dir() -> PathBuf {
    if let Some(p) = env::var_os("PAWLINE_LLAMA_SRC") {
        let p = PathBuf::from(p);
        assert!(p.join("include/llama.h").is_file(), "PAWLINE_LLAMA_SRC has no include/llama.h");
        return p;
    }
    let here = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let p = here.join("../vendor/llama.cpp");
    assert!(
        p.join("include/llama.h").is_file(),
        "no vendored llama.cpp: set PAWLINE_LLAMA_SRC or add vendor/llama.cpp"
    );
    p
}

fn find_lib_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("libllama.a") || name.starts_with("libggml.a") || name.starts_with("libmtmd.a") {
                    if let Some(parent) = p.parent() {
                        if !out.iter().any(|d| d == parent) {
                            out.push(parent.to_path_buf());
                        }
                    }
                }
            }
        }
    }
    out
}
