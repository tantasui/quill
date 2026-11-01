use std::fmt;

/// A design value that failed validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenError {
    kind: &'static str,
    input: String,
    reason: String,
}

impl TokenError {
    pub(crate) fn new(kind: &'static str, input: &str, reason: &str) -> Self {
        TokenError {
            kind,
            input: input.to_string(),
            reason: reason.to_string(),
        }
    }
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} `{}`: {}", self.kind, self.input, self.reason)
    }
}

impl std::error::Error for TokenError {}
