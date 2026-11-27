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
