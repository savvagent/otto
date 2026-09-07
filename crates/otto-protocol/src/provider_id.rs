//! Stable identifier for an LLM provider.

use thiserror::Error;

/// Error returned when [`ProviderId::new`] rejects its input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ProviderIdError(String);

/// Stable identifier for an LLM provider.
///
/// External callers must use [`ProviderId::new`] to construct a value; the inner
/// field is `pub(crate)` so direct tuple construction is only available within
/// this crate.
///
/// # Examples
///
/// ```
/// use otto_protocol::ProviderId;
///
/// let id = ProviderId::new("anthropic").unwrap();
/// assert_eq!(id.as_str(), "anthropic");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderId(pub(crate) String);

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl ProviderId {
    /// Construct a validated provider id. Must be non-empty and consist
    /// only of `[a-z0-9_-]` characters, starting with `[a-z]`.
    pub fn new(s: impl Into<String>) -> Result<Self, ProviderIdError> {
        let s: String = s.into();
        if s.is_empty() {
            return Err(ProviderIdError("provider id must be non-empty".into()));
        }
        let mut chars = s.chars();
        let first = chars.next().unwrap();
        if !first.is_ascii_lowercase() {
            return Err(ProviderIdError(format!(
                "provider id must start with [a-z] — got {s:?}"
            )));
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(ProviderIdError(format!(
                "provider id must match [a-z][a-z0-9_-]* — got {s:?}"
            )));
        }
        Ok(Self(s))
    }

    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_anthropic() {
        let id = ProviderId::new("anthropic").unwrap();
        assert_eq!(id.as_str(), "anthropic");
    }

    #[test]
    fn accepts_lowercase_with_hyphen() {
        let id = ProviderId::new("provider-gemini").unwrap();
        assert_eq!(id.as_str(), "provider-gemini");
    }

    #[test]
    fn accepts_lowercase_with_digit() {
        let id = ProviderId::new("gpt4").unwrap();
        assert_eq!(id.as_str(), "gpt4");
    }

    #[test]
    fn rejects_empty() {
        let err = ProviderId::new("").unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn rejects_uppercase_first() {
        let err = ProviderId::new("Anthropic").unwrap_err();
        assert!(err.to_string().contains("[a-z]"));
    }

    #[test]
    fn rejects_uppercase_middle() {
        let err = ProviderId::new("anthropicAI").unwrap_err();
        assert!(err.to_string().contains("[a-z][a-z0-9_-]*"));
    }

    #[test]
    fn rejects_colon() {
        let err = ProviderId::new("internal:provider").unwrap_err();
        assert!(err.to_string().contains("[a-z][a-z0-9_-]*"));
    }

    #[test]
    fn as_str_roundtrips() {
        let id = ProviderId::new("gemini").unwrap();
        assert_eq!(id.as_str(), "gemini");
    }

    #[test]
    fn provider_id_error_displays() {
        let err = ProviderIdError("provider id must be non-empty".into());
        assert_eq!(format!("{err}"), "provider id must be non-empty");
    }
}
