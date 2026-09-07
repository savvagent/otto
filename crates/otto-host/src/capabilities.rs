//! Per-provider and per-model capability metadata. Carried into the host
//! via `HostConfig::providers` (see config.rs::ProviderRegistration).
//! Plugins build these from their hardcoded model lists; the host treats
//! them as read-only data and never mutates capability records itself.

use otto_protocol::ProviderId;

/// Coarse cost category for a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostTier {
    /// No usage charge (e.g. local models).
    Free,
    /// Below-average cost per token.
    Cheap,
    /// Typical commercial model pricing.
    Standard,
    /// High-capability frontier model pricing.
    Premium,
}

/// Per-model capability metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// Bare model id (e.g. `"claude-opus-4-7"`).
    pub id: String,
    /// Human-readable name for display in the UI.
    pub display_name: String,
    /// Whether the model accepts image inputs.
    pub supports_vision: bool,
    /// Whether the model accepts audio inputs.
    pub supports_audio: bool,
    /// Maximum input context window in tokens.
    pub context_window: usize,
    /// Rough cost category for the model.
    pub cost_tier: CostTier,
}

/// A short alias that maps a bare name to a specific provider + model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAlias {
    /// Short name (e.g. `"opus"`).
    pub alias: String,
    /// Provider that owns this model.
    pub provider: ProviderId,
    /// Fully-qualified model id on that provider.
    pub model: String,
}

/// Error returned when [`ProviderCapabilities::new`] is given invalid arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilitiesError {
    /// The `models` list was empty; at least one model is required.
    EmptyModels,
    /// `default_model` was not found in the `models` list.
    DefaultModelNotInList {
        /// The `default_model` value that was not present.
        default: String,
    },
}

impl std::fmt::Display for CapabilitiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyModels => write!(f, "ProviderCapabilities requires at least one model"),
            Self::DefaultModelNotInList { default } => {
                write!(f, "default_model '{default}' is not in the models list")
            }
        }
    }
}

impl std::error::Error for CapabilitiesError {}

/// All capability metadata for one provider.
///
/// Fields are private; construct via [`ProviderCapabilities::new`] which
/// validates that `default_model` exists in `models`.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    /// Ordered list of models offered by the provider.
    models: Vec<ModelCapabilities>,
    /// Id of the model to use when none is specified.
    default_model: String,
}

impl ProviderCapabilities {
    /// Construct, validating that `default_model` exists in `models` and that
    /// the list is non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`CapabilitiesError::EmptyModels`] if `models` is empty.
    /// Returns [`CapabilitiesError::DefaultModelNotInList`] if `default_model`
    /// does not appear in `models`.
    ///
    /// # Examples
    ///
    /// ```
    /// use otto_host::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};
    ///
    /// let model = ModelCapabilities {
    ///     id: "claude-haiku-4-5".into(),
    ///     display_name: "Claude Haiku 4.5".into(),
    ///     supports_vision: false,
    ///     supports_audio: false,
    ///     context_window: 200_000,
    ///     cost_tier: CostTier::Cheap,
    /// };
    /// let caps = ProviderCapabilities::new(vec![model], "claude-haiku-4-5".into())
    ///     .expect("valid caps");
    /// assert_eq!(caps.default().id, "claude-haiku-4-5");
    /// ```
    pub fn new(
        models: Vec<ModelCapabilities>,
        default_model: String,
    ) -> Result<Self, CapabilitiesError> {
        if models.is_empty() {
            return Err(CapabilitiesError::EmptyModels);
        }
        if !models.iter().any(|m| m.id == default_model) {
            return Err(CapabilitiesError::DefaultModelNotInList {
                default: default_model,
            });
        }
        Ok(Self {
            models,
            default_model,
        })
    }

    /// Look up a model by id, returning `None` if not found.
    pub fn model(&self, id: &str) -> Option<&ModelCapabilities> {
        self.models.iter().find(|m| m.id == id)
    }

    /// All models offered by this provider.
    pub fn models(&self) -> &[ModelCapabilities] {
        &self.models
    }

    /// The id of the default model.
    pub fn default_model_id(&self) -> &str {
        &self.default_model
    }

    /// Return the default model's capabilities.
    ///
    /// The constructor invariant guarantees `default_model` exists in `models`,
    /// so the internal `expect` cannot fire in practice.
    pub fn default(&self) -> &ModelCapabilities {
        // Constructor invariant guarantees this lookup succeeds.
        self.model(&self.default_model)
            .expect("default_model exists by constructor invariant")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_protocol::ProviderId;

    fn anthropic() -> ProviderId {
        ProviderId::new("anthropic").unwrap()
    }

    fn opus_caps() -> ModelCapabilities {
        ModelCapabilities {
            id: "claude-opus-4-7".into(),
            display_name: "Claude Opus 4.7".into(),
            supports_vision: true,
            supports_audio: false,
            context_window: 200_000,
            cost_tier: CostTier::Premium,
        }
    }

    #[test]
    fn provider_caps_lookup_by_id() {
        let caps = ProviderCapabilities::new(vec![opus_caps()], "claude-opus-4-7".into())
            .expect("valid caps");
        assert_eq!(caps.model("claude-opus-4-7").unwrap().id, "claude-opus-4-7");
        assert!(caps.model("not-a-model").is_none());
        assert_eq!(caps.default().id, "claude-opus-4-7");
    }

    #[test]
    fn model_alias_struct_is_constructible() {
        let alias = ModelAlias {
            alias: "opus".into(),
            provider: anthropic(),
            model: "claude-opus-4-7".into(),
        };
        assert_eq!(alias.alias, "opus");
    }

    #[test]
    fn empty_models_returns_error() {
        let err = ProviderCapabilities::new(vec![], "missing".into()).unwrap_err();
        assert_eq!(err, CapabilitiesError::EmptyModels);
    }

    #[test]
    fn default_not_in_list_returns_error() {
        let err = ProviderCapabilities::new(vec![opus_caps()], "not-here".into()).unwrap_err();
        assert!(
            matches!(err, CapabilitiesError::DefaultModelNotInList { ref default } if default == "not-here")
        );
    }
}
