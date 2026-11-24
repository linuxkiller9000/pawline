//! Optional assist path: when the deterministic parser cannot handle an
//! input, a pluggable assist (the embedded Needle3 runtime, or anything else)
//! may PROPOSE directives. Proposals re-enter the normal pipeline at
//! validation — the assist never touches execution, and the validator's
//! confinement rules apply to model output exactly as they apply to parser
//! output.

use crate::error::PawError;
use crate::plan::Directive;

pub trait Assist {
    /// Propose directives for an input the parser refused.
    fn propose(&self, input: &str) -> Result<Vec<Directive>, PawError>;
}
