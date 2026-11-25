//! Native-Rust Needle assist. Inference runs in Rust code in this workspace
//! (vendored needle-rs, MIT) over the embedded Needle3 weights. No Cactus
//! runtime, no FFI, no C++ link.
//!
//! The reply shape matches what `pawline-needle`'s parser expects, so the same
//! validation and confirmation pipeline applies: nothing here is trusted.

use needle_infer::v3_engine::{extract_tool_call, PrefixState, V3Engine, V3Options};

/// The model weights, compiled into the binary.
pub static NEEDLE3_CACT: &[u8] = include_bytes!("../../pawline-needle/needle3.cact");

#[derive(Debug)]
pub struct NeedleError(pub String);

pub struct NeedleNative {
    engine: V3Engine,
    tools_json: String,
    prefix: PrefixState,
}

impl NeedleNative {
    /// Load the embedded weights and bind the tool schema.
    pub fn embedded(tools_json: &str) -> Result<Self, NeedleError> {
        let engine = V3Engine::from_bytes(NEEDLE3_CACT.to_vec())
            .map_err(|e| NeedleError(format!("needle load: {e:?}")))?;
        let prefix = engine.make_prefix(tools_json, None);
        Ok(Self { engine, tools_json: tools_json.to_string(), prefix })
    }

    /// Propose tool calls for one input. Stateless: each call is a fresh turn.
    /// Returns a JSON envelope: {"type":"call","success":bool,
    /// "function_calls":[...],"confidence":f} or success=false with "error".
    pub fn propose(&self, input: &str, max_new_tokens: usize) -> String {
        let opts = V3Options {
            max_new_tokens,
            constrain: true,
            ..V3Options::default()
        };
        let res = self.engine.generate_prefixed(&self.prefix, input, &self.tools_json, &opts);
        let conf = self.engine.confidence_for(input, &self.tools_json, &res.text);
        let calls = extract_tool_call(&res.text)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        match calls {
            Some(serde_json::Value::Array(a)) if !a.is_empty() => serde_json::json!({
                "type": "call", "success": true,
                "function_calls": a, "confidence": conf,
            })
            .to_string(),
            _ => serde_json::json!({
                "type": "respond", "success": false,
                "error": "model produced no tool call", "confidence": conf,
            })
            .to_string(),
        }
    }
}
