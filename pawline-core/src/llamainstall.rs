//! "Get me a llama-server" without making the user do it themselves.
//!
//! The rule this module follows: Pawline never downloads anything because it
//! decided to. Detection comes first, and when nothing is found the offer is a
//! card that names the source, the exact size, the destination and the
//! checksum, and nothing happens until the user says yes. The download is the
//! official llama.cpp release build for this machine, extracted under
//! `~/.local/share/pawline/llama/<tag>/`, so it needs no root, touches no
//! system directory, and can be removed by deleting one folder.
//!
//! The release binary is the CPU build, not the CUDA/Vulkan/ROCm/SYCL ones: it
//! carries one ggml backend per instruction set (x64, sse42, sandybridge, ...)
//! and picks at run time, which is what makes it work on a CPU with no AVX.
//!
//! All the I/O is injected (fetch, extract, check) so the decision logic here is
//! testable without a network, a tarball or a real binary.

use std::path::{Path, PathBuf};

/// The llama.cpp release Pawline installs. Pinned: a moving target would change
/// the checksum under the user's feet.
pub const TAG: &str = "b12811";

/// Everything the card and the download need about one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub tag: &'static str,
    pub asset: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// The build Pawline offers for this machine, or None when it has none.
/// Only the CPU build for Linux x86-64 is pinned today.
pub fn candidate(os: &str, arch: &str) -> Option<Candidate> {
    if os == "linux" && (arch == "x86_64" || arch == "amd64") {
        Some(Candidate {
            tag: TAG,
            asset: "llama-b12811-bin-ubuntu-x64.tar.gz",
            url: "https://github.com/ggml-org/llama.cpp/releases/download/b12811/llama-b12811-bin-ubuntu-x64.tar.gz",
            sha256: "d6129b610ad889c80545177a462f49b7960fbddecccdb82a176150a9f0cfede0",
            bytes: 23_491_893,
        })
    } else {
        None
    }
}

/// Where Pawline puts it: one folder under its own share directory, no root.
pub fn dest_dir(home: &Path, tag: &str) -> PathBuf {
    home.join(".local/share/pawline/llama").join(tag)
}

/// A llama-server Pawline installed earlier, if it is still there.
pub fn installed_at(home: &Path, tag: &str) -> Option<PathBuf> {
    let p = dest_dir(home, tag).join(format!("llama-{tag}")).join("llama-server");
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Find a usable llama-server in the order that costs the user least:
/// one Pawline installed before, then one on PATH, then one sitting next to the
/// model file the user pointed at.
pub fn find_existing(
    home: &Path,
    has_on_path: &dyn Fn(&str) -> bool,
    model_file: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = installed_at(home, TAG) {
        return Some(p);
    }
    if has_on_path("llama-server") {
        return Some(PathBuf::from("llama-server"));
    }
    if let Some(m) = model_file {
        if let Some(dir) = m.parent() {
            for cand in [dir.join("llama-server"), dir.join("llama-b12811").join("llama-server")] {
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// The text of the card, exactly as the user sees it before anything happens.
pub fn card_text(c: &Candidate, home: &Path) -> String {
    let mb = c.bytes as f64 / (1024.0 * 1024.0);
    format!(
        "No llama-server was found on this computer. Pawline can install one for you.\n\
         \n\
         Source: the official llama.cpp release {}, CPU build, from github.com/ggml-org/llama.cpp\n\
         Download: {:.1} MB ({})\n\
         Checked against sha256 {}\n\
         Goes to: {} (your home folder, no root, nothing system-wide)\n\
         \n\
         It is the CPU build, so it runs on a processor without AVX by picking the right kernel at start.\n\
         Nothing is downloaded until you say yes, and deleting that one folder removes it.",
        c.tag,
        mb,
        c.asset,
        c.sha256,
        dest_dir(home, c.tag).display()
    )
}

/// Lowercase hex sha-256 of the bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Does this download match the pinned checksum? A size that differs is a
/// refusal on its own: a truncated download must never be extracted.
pub fn verify(bytes: &[u8], c: &Candidate) -> Result<(), String> {
    if bytes.len() as u64 != c.bytes {
        return Err(format!(
            "the download is {} bytes, expected {} - not extracting a partial file",
            bytes.len(),
            c.bytes
        ));
    }
    let got = sha256_hex(bytes);
    if got != c.sha256 {
        return Err(format!("the download does not match its checksum (got {got})"));
    }
    Ok(())
}

/// Fetch, check, extract, and prove the binary starts. Each step is injected so
/// this is testable without a network or a real tarball.
pub fn install(
    c: &Candidate,
    home: &Path,
    fetch: impl Fn(&str) -> Result<Vec<u8>, String>,
    extract: impl Fn(&[u8], &Path) -> Result<(), String>,
    check_binary: impl Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let dir = dest_dir(home, c.tag);
    let bytes = fetch(c.url)?;
    verify(&bytes, c)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    extract(&bytes, &dir)?;
    let bin = dir.join(format!("llama-{}", c.tag)).join("llama-server");
    if !bin.is_file() {
        return Err(format!("the archive did not contain {}", bin.display()));
    }
    check_binary(&bin)?;
    Ok(bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/someone")
    }

    #[test]
    fn only_the_linux_cpu_build_is_offered_and_it_is_pinned() {
        let c = candidate("linux", "x86_64").expect("linux x86_64 has a build");
        assert_eq!(c.tag, "b12811");
        assert!(c.url.contains("llama-b12811-bin-ubuntu-x64.tar.gz"));
        assert!(c.url.starts_with("https://github.com/ggml-org/llama.cpp/releases/download/"));
        assert_eq!(c.sha256.len(), 64);
        assert!(c.bytes > 20_000_000 && c.bytes < 30_000_000);
        assert!(candidate("linux", "aarch64").is_none());
        assert!(candidate("windows", "x86_64").is_none());
        assert!(candidate("macos", "aarch64").is_none());
    }

    #[test]
    fn the_card_names_source_size_destination_and_checksum() {
        let c = candidate("linux", "x86_64").unwrap();
        let t = card_text(&c, &home());
        assert!(t.contains("github.com/ggml-org/llama.cpp"));
        assert!(t.contains("22.4 MB"), "size in the card: {t}");
        assert!(t.contains("/home/someone/.local/share/pawline/llama/b12811"));
        assert!(t.contains(&c.sha256[..16]));
        assert!(t.contains("no root"));
        assert!(t.contains("Nothing is downloaded until you say yes"));
    }

    #[test]
    fn a_wrong_checksum_or_a_short_file_is_refused() {
        let c = candidate("linux", "x86_64").unwrap();
        let short = vec![0u8; 10];
        let e = verify(&short, &c).unwrap_err();
        assert!(e.contains("partial"), "{e}");
        let mut same_size = vec![0u8; c.bytes as usize];
        same_size[0] = 7;
        let e = verify(&same_size, &c).unwrap_err();
        assert!(e.contains("checksum"), "{e}");
    }

    #[test]
    fn nothing_is_extracted_when_the_download_does_not_verify() {
        let c = candidate("linux", "x86_64").unwrap();
        let extracted = std::cell::Cell::new(false);
        let r = install(
            &c,
            &home(),
            |_| Ok(vec![1, 2, 3]),
            |_, _| {
                extracted.set(true);
                Ok(())
            },
            |_| Ok(()),
        );
        assert!(r.is_err());
        assert!(!extracted.get(), "a bad download must not reach the extractor");
    }

    #[test]
    fn a_verified_download_is_extracted_checked_and_returned() {
        let bytes: Vec<u8> = b"pretend tarball".to_vec();
        let sha: &'static str = Box::leak(sha256_hex(&bytes).into_boxed_str());
        let c = Candidate {
            tag: TAG,
            asset: "fake.tar.gz",
            url: "https://example.invalid/fake.tar.gz",
            sha256: sha,
            bytes: bytes.len() as u64,
        };
        let tmp = std::env::temp_dir().join(format!("pawline-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let checked = std::cell::Cell::new(false);
        let r = install(
            &c,
            &tmp,
            |_| Ok(bytes.clone()),
            |_, d| {
                let bin_dir = d.join(format!("llama-{TAG}"));
                std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
                std::fs::write(bin_dir.join("llama-server"), b"#!/bin/sh\n").map_err(|e| e.to_string())
            },
            |bin| {
                checked.set(true);
                assert!(bin.is_file(), "the check runs on the extracted binary");
                Ok(())
            },
        )
        .expect("verified bytes install");
        assert!(checked.get());
        assert_eq!(r, dest_dir(&tmp, TAG).join(format!("llama-{TAG}")).join("llama-server"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn an_existing_install_is_found_before_anything_else() {
        let tmp = std::env::temp_dir().join(format!("pawline-find-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = dest_dir(&tmp, TAG).join(format!("llama-{TAG}")).join("llama-server");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let found = find_existing(&tmp, &|_| true, None).unwrap();
        assert_eq!(found, bin);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn path_then_next_to_the_model_are_the_next_places() {
        let empty = std::env::temp_dir().join("pawline-no-install-here");
        let on_path = find_existing(&empty, &|_| true, None).unwrap();
        assert_eq!(on_path, PathBuf::from("llama-server"));
        let none = find_existing(&empty, &|_| false, None);
        assert!(none.is_none());

        let dir = std::env::temp_dir().join(format!("pawline-model-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("smolvlm.gguf");
        std::fs::write(&model, b"x").unwrap();
        let beside = dir.join("llama-server");
        std::fs::write(&beside, b"#!/bin/sh\n").unwrap();
        let found = find_existing(&empty, &|_| false, Some(&model)).unwrap();
        assert_eq!(found, beside);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
