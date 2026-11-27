//! End-to-end check of the chat path against a real local OpenAI-compatible
//! server. Needs the mock running:
//!     python3 tools/mock_llm.py            # listens on 127.0.0.1:18123
//! then:
//!     cargo test -p pawline-core --test mock_local_llm -- --ignored
//! This is the same shape as Ollama / llama.cpp / LM Studio on the Dell.

use pawline_core::imagegen::UreqHttp;
use pawline_core::providers::ChatProvider;
use pawline_core::textfallback::chat;

#[test]
#[ignore = "needs tools/mock_llm.py running on 127.0.0.1:18123"]
fn chat_roundtrip_against_a_local_openai_compatible_server() {
    let http = UreqHttp::new(std::time::Duration::from_secs(10));
    let provider = ChatProvider::openai_compatible(
        "Mock local LLM",
        "http://127.0.0.1:18123",
        "mock-model-1",
        None,
    );
    assert!(provider.local, "loopback must classify as local");
    let ctx = vec!["earlier question".to_string(), "earlier answer".to_string()];
    let a = chat(&http, &provider, &ctx, "hello mock", &|| false, &|| true).unwrap();
    assert_eq!(a, "mock reply to: hello mock");
}

#[test]
#[ignore = "needs tools/mock_llm.py running on 127.0.0.1:18123"]
fn vision_roundtrip_against_a_local_openai_compatible_server() {
    let http = UreqHttp::new(std::time::Duration::from_secs(10));
    let provider = ChatProvider::openai_compatible("Mock local VLM", "http://127.0.0.1:18123", "mock-vlm-1", None);
    // A real (tiny) PNG signature plus payload; the mock does not parse it.
    let img = pawline_core::textfallback::ChatImage::from_bytes(b"\x89PNG\r\n\x1a\nfake-pixels").unwrap();
    let a = pawline_core::textfallback::chat_with_image(&http, &provider, &img, "what is in this picture", &|| false, &|| true).unwrap();
    assert!(a.starts_with("mock reply to: "), "{a}");
}

/// Live test against a REAL server: set PAWLINE_LIVE_URL (e.g.
/// http://127.0.0.1:18124) and PAWLINE_LIVE_MODEL (the loaded model's name).
/// cargo test -p pawline-core --test mock_local_llm live_ -- --ignored
#[test]
#[ignore = "needs PAWLINE_LIVE_URL and PAWLINE_LIVE_MODEL set"]
fn live_chat_roundtrip() {
    let url = std::env::var("PAWLINE_LIVE_URL").expect("PAWLINE_LIVE_URL");
    let model = std::env::var("PAWLINE_LIVE_MODEL").expect("PAWLINE_LIVE_MODEL");
    let http = UreqHttp::new(std::time::Duration::from_secs(120));
    let provider = ChatProvider::openai_compatible("Live local LLM", &url, &model, None);
    let t0 = std::time::Instant::now();
    let a = chat(&http, &provider, &[], "Say the word pineapple and nothing else.", &|| false, &|| true).unwrap();
    let dt = t0.elapsed();
    println!("LIVE ANSWER in {dt:?}: {a}");
    assert!(a.to_lowercase().contains("pineapple"), "{a}");
}

#[test]
#[ignore = "needs PAWLINE_LIVE_URL and PAWLINE_LIVE_MODEL set"]
fn live_vision_roundtrip() {
    let url = std::env::var("PAWLINE_LIVE_URL").expect("PAWLINE_LIVE_URL");
    let model = std::env::var("PAWLINE_LIVE_MODEL").expect("PAWLINE_LIVE_MODEL");
    let http = UreqHttp::new(std::time::Duration::from_secs(300));
    let provider = ChatProvider::openai_compatible("Live local VLM", &url, &model, None);
    // A real 1x1 red PNG (the server decodes the image for real).
    const RED_PIXEL_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    fn b64d(s: &str) -> Vec<u8> {
        let val = |c: u8| -> u32 { match c { b'A'..=b'Z' => (c - b'A') as u32, b'a'..=b'z' => (c - b'a' + 26) as u32, b'0'..=b'9' => (c - b'0' + 52) as u32, b'+' => 62, b'/' => 63, _ => 0 } };
        let mut out = Vec::new();
        let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
        for chunk in bytes.chunks(4) {
            let mut n = 0u32;
            for (i, &c) in chunk.iter().enumerate() { n |= val(c) << (18 - 6 * i); }
            out.push((n >> 16) as u8);
            if chunk.len() > 2 { out.push((n >> 8) as u8); }
            if chunk.len() > 3 { out.push(n as u8); }
        }
        out
    }
    let png = match std::env::var("PAWLINE_LIVE_IMAGE") {
        Ok(path) => std::fs::read(path).unwrap(),
        Err(_) => b64d(RED_PIXEL_B64),
    };
    let img = pawline_core::textfallback::ChatImage::from_bytes(&png).unwrap();
    let t0 = std::time::Instant::now();
    let r = pawline_core::textfallback::chat_with_image(&http, &provider, &img, "Describe the colors and shapes in this image. Answer briefly.", &|| false, &|| true);
    println!("LIVE VISION in {:?}: {:?}", t0.elapsed(), r);
    // A corrupt image may be refused by the server; the request itself must
    // have been accepted and answered (any 200-level parsed answer or a named error).
    match r {
        Ok(a) => assert!(!a.is_empty()),
        Err(e) => panic!("transport-level failure (server should answer something): {e}"),
    }
}
