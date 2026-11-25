//! Where optional voice files live and which exact bytes are accepted.
//! Pins are compiled in: files next to the binary are never trusted by name.
use std::path::PathBuf;

/// (file name, sha256) in load order. Moonshine v0.1.6 prebuilt libs plus the
/// Ubuntu 22.04 gcc-12 12.3.0-1ubuntu1~22.04.3 runtime they need.
pub const LIBS: [(&str, &str); 4] = [
    ("libgcc_s.so.1", "fc9d43b2f6c20e53b009238f767c5b949d202389e20de9e202ea684b4ba3729a"),
    ("libstdc++.so.6", "ff0825e113603c3866680d5d52216bc6d8eedf3a59f52a0aef67ff01994db128"),
    ("libonnxruntime.so.1", "13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b"),
    ("libmoonshine.so", "85be190fe5f8764c65d81f5c99c33a68327c5570534e35b62a4fafa5cdaa6dc1"),
];

pub fn voice_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PAWLINE_VOICE_DIR") {
        return PathBuf::from(d);
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join("pawline/voice");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/pawline/voice")
}
pub fn model_dir() -> PathBuf {
    voice_dir().join("models/en")
}
