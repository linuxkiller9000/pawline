use std::fmt;

/// Every failure Pawline can report. No silent skips: each stage either
/// produces a value or one of these, with a message meant for the user.
#[derive(Debug)]
pub enum PawError {
    /// The parser could not understand the request at all.
    Parse(String),
    /// A reference like "it" or a folder name could not be resolved.
    UnknownReference(String),
    /// A path would escape the allowed root, or is otherwise unsafe.
    UnsafePath(String),
    /// A planned operation failed validation (missing parent, conflict, ...).
    Validation(String),
    /// The plan contains something that needs the user's confirmation first.
    ConfirmationRequired(String),
    /// The execution backend failed an operation.
    Exec(String),
}

impl fmt::Display for PawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PawError::Parse(m) => write!(f, "I couldn't understand that: {m}"),
            PawError::UnknownReference(m) => write!(f, "I don't know what \"{m}\" refers to."),
            PawError::UnsafePath(m) => write!(f, "That path isn't allowed: {m}"),
            PawError::Validation(m) => write!(f, "The plan doesn't check out: {m}"),
            PawError::ConfirmationRequired(m) => write!(f, "Needs confirmation: {m}"),
            PawError::Exec(m) => write!(f, "Execution failed: {m}"),
        }
    }
}

impl std::error::Error for PawError {}

impl From<std::io::Error> for PawError {
    fn from(e: std::io::Error) -> Self {
        PawError::Exec(e.to_string())
    }
}
