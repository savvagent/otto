//! Resolve a `OTTO_MODEL`-shaped value against the set of connected
//! providers. Accepts both legacy bare-model form ("claude-opus-4-7")
//! and the new "provider/model" form ("anthropic/claude-opus-4-7").
//! Pure function; no I/O. The caller is responsible for surfacing the
//! returned warnings as styled notes.

use otto_protocol::ProviderId;

use crate::capabilities::ProviderCapabilities;

/// What `resolve_legacy_model` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LegacyModelResolution {
    /// Exact match found via `provider/model` form.
    Resolved {
        /// The matched provider.
        provider: ProviderId,
        /// The matched model id.
        model: String,
    },
    /// Bare-model resolved unambiguously across all connected providers.
    ResolvedFromBare {
        /// The provider that exposes this model.
        provider: ProviderId,
        /// The matched model id.
        model: String,
        /// User-facing note describing what was resolved.
        note: String,
    },
    /// Multiple providers expose this model id; fell back to default.
    Ambiguous {
        /// All (provider, model) pairs that matched.
        candidates: Vec<(ProviderId, String)>,
        /// User-facing note explaining the ambiguity.
        note: String,
    },
    /// `provider/model` named a known provider but unknown model.
    Unknown {
        /// User-facing note describing the problem.
        note: String,
    },
    /// `provider/model` named a provider that isn't connected.
    UnknownProvider {
        /// The provider id that was requested but is not connected.
        provider: ProviderId,
        /// User-facing note describing the problem.
        note: String,
    },
    /// Empty / no override.
    NoOverride,
}

/// One per connected provider, by reference, so the resolver does not
/// need to hold an owned snapshot.
pub struct ProviderView<'a> {
    /// The provider's stable identifier.
    pub id: &'a ProviderId,
    /// The provider's capability metadata.
    pub capabilities: &'a ProviderCapabilities,
}

/// Resolve a `OTTO_MODEL` value against the connected provider set.
///
/// Accepts both the legacy bare-model form (`"claude-opus-4-7"`) and
/// the qualified `"provider/model"` form (`"anthropic/claude-opus-4-7"`).
/// The function is pure — no I/O, no environment reads, no async.
///
/// # Examples
///
/// ```
/// use otto_host::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};
/// use otto_host::router::{resolve_legacy_model, LegacyModelResolution, ProviderView};
/// use otto_protocol::ProviderId;
///
/// let id = ProviderId::new("anthropic").unwrap();
/// let caps = ProviderCapabilities::new(
///     vec![ModelCapabilities {
///         id: "claude-opus-4-7".into(),
///         display_name: "Claude Opus 4.7".into(),
///         supports_vision: true,
///         supports_audio: false,
///         context_window: 200_000,
///         cost_tier: CostTier::Premium,
///     }],
///     "claude-opus-4-7".into(),
/// )
/// .expect("valid caps");
/// let views = vec![ProviderView { id: &id, capabilities: &caps }];
///
/// let r = resolve_legacy_model("anthropic/claude-opus-4-7", &views);
/// assert!(matches!(r, LegacyModelResolution::Resolved { .. }));
/// ```
pub fn resolve_legacy_model(raw: &str, providers: &[ProviderView<'_>]) -> LegacyModelResolution {
    let raw = raw.trim();
    if raw.is_empty() {
        return LegacyModelResolution::NoOverride;
    }
    if let Some((provider_part, model_part)) = raw.split_once('/') {
        let pid = match ProviderId::new(provider_part) {
            Ok(p) => p,
            Err(_) => {
                return LegacyModelResolution::Unknown {
                    note: format!(
                        "OTTO_MODEL='{raw}' has invalid provider id '{provider_part}'; \
                         falling back to default"
                    ),
                };
            }
        };
        let Some(view) = providers.iter().find(|p| p.id == &pid) else {
            return LegacyModelResolution::UnknownProvider {
                provider: pid.clone(),
                note: format!(
                    "OTTO_MODEL='{raw}' names provider '{}' which is not connected; \
                     falling back to default",
                    pid.as_str()
                ),
            };
        };
        if view.capabilities.model(model_part).is_none() {
            return LegacyModelResolution::Unknown {
                note: format!(
                    "OTTO_MODEL='{raw}' names model '{model_part}' \
                     which provider '{}' does not expose; \
                     falling back to {}'s default model",
                    pid.as_str(),
                    pid.as_str()
                ),
            };
        }
        return LegacyModelResolution::Resolved {
            provider: pid,
            model: model_part.into(),
        };
    }
    // Bare-model form: scan all providers.
    let mut hits: Vec<(ProviderId, String)> = providers
        .iter()
        .filter(|v| v.capabilities.model(raw).is_some())
        .map(|v| (v.id.clone(), raw.to_string()))
        .collect();
    match hits.len() {
        0 => LegacyModelResolution::Unknown {
            note: format!(
                "OTTO_MODEL='{raw}' did not match any connected provider's model; \
                 falling back to default"
            ),
        },
        1 => {
            let (provider, model) = hits.remove(0);
            let note = format!(
                "OTTO_MODEL='{raw}' resolved to '{}/{model}'",
                provider.as_str()
            );
            LegacyModelResolution::ResolvedFromBare {
                provider,
                model,
                note,
            }
        }
        _ => {
            let candidates_str = hits
                .iter()
                .map(|(p, _)| p.as_str().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            LegacyModelResolution::Ambiguous {
                candidates: hits,
                note: format!(
                    "OTTO_MODEL='{raw}' is ambiguous: matches providers [{candidates_str}]. \
                     Falling back to default; switch to 'provider/model' form to disambiguate."
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};

    fn caps(models: &[&str]) -> ProviderCapabilities {
        ProviderCapabilities::new(
            models
                .iter()
                .map(|id| ModelCapabilities {
                    id: id.to_string(),
                    display_name: id.to_string(),
                    supports_vision: false,
                    supports_audio: false,
                    context_window: 0,
                    cost_tier: CostTier::Standard,
                })
                .collect(),
            models[0].into(),
        )
        .expect("valid test caps")
    }

    #[test]
    fn empty_input_means_no_override() {
        let views: Vec<ProviderView> = vec![];
        assert_eq!(
            resolve_legacy_model("", &views),
            LegacyModelResolution::NoOverride
        );
    }

    #[test]
    fn qualified_form_resolves() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let views = vec![ProviderView {
            id: &a_id,
            capabilities: &a_caps,
        }];
        let r = resolve_legacy_model("anthropic/claude-opus-4-7", &views);
        assert_eq!(
            r,
            LegacyModelResolution::Resolved {
                provider: a_id,
                model: "claude-opus-4-7".into(),
            }
        );
    }

    #[test]
    fn bare_form_resolves_when_one_match() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let g_id = ProviderId::new("gemini").unwrap();
        let g_caps = caps(&["gemini-pro"]);
        let views = vec![
            ProviderView {
                id: &a_id,
                capabilities: &a_caps,
            },
            ProviderView {
                id: &g_id,
                capabilities: &g_caps,
            },
        ];
        match resolve_legacy_model("claude-opus-4-7", &views) {
            LegacyModelResolution::ResolvedFromBare {
                provider, model, ..
            } => {
                assert_eq!(provider.as_str(), "anthropic");
                assert_eq!(model, "claude-opus-4-7");
            }
            other => panic!("expected ResolvedFromBare, got {other:?}"),
        }
    }

    #[test]
    fn bare_form_ambiguous_falls_back() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let g_id = ProviderId::new("gemini").unwrap();
        let caps_both = caps(&["shared-model"]);
        let views = vec![
            ProviderView {
                id: &a_id,
                capabilities: &caps_both,
            },
            ProviderView {
                id: &g_id,
                capabilities: &caps_both,
            },
        ];
        match resolve_legacy_model("shared-model", &views) {
            LegacyModelResolution::Ambiguous { candidates, .. } => {
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unknown_provider_returns_unknown_provider() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["m"]);
        let views = vec![ProviderView {
            id: &a_id,
            capabilities: &a_caps,
        }];
        match resolve_legacy_model("openai/gpt-5", &views) {
            LegacyModelResolution::UnknownProvider { provider, .. } => {
                assert_eq!(provider.as_str(), "openai");
            }
            other => panic!("expected UnknownProvider, got {other:?}"),
        }
    }

    #[test]
    fn qualified_form_unknown_model_returns_unknown() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let views = vec![ProviderView {
            id: &a_id,
            capabilities: &a_caps,
        }];
        match resolve_legacy_model("anthropic/nope", &views) {
            LegacyModelResolution::Unknown { .. } => {}
            other => panic!("expected Unknown, got {other:?}"),
        }
    }
}
