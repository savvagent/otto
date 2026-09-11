//! `skill` in-process tool handler — level 2 of progressive disclosure.
//!
//! The model calls this with a name drawn from the level-1 catalog; the
//! handler resolves the skill, checks the level-3 trust gate, and
//! returns the full `SKILL.md` body plus the absolute skill root so
//! relative references in the body resolve for on-demand reads.
//!
//! Unlike the `task` tool this handler needs no `Host` access at all —
//! it reads the index and returns text. That is deliberate: it keeps
//! the skill path clear of the host-swap `RwLock` entirely. The only
//! lock touched is [`SkillIndex`]'s own, and `SkillIndex::get` clones
//! the `Arc` out and drops its guard before returning, so no guard is
//! ever held across an `.await` here.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use otto_plugin::{InProcessToolHandler, InProcessToolHandlerArc};
use otto_protocol::ToolDef;
use serde::Deserialize;
use serde_json::Value;

use crate::plugin::builtin::user_skills::index::SkillIndex;
use crate::plugin::builtin::user_skills::trust::{self, SkillTrust};
use crate::plugin::builtin::user_slash_commands::TrustMap;

/// JSON shape expected from the model when it calls the `skill` tool.
#[derive(Debug, Deserialize)]
struct SkillInput {
    /// The skill slug, drawn from the tool's enum schema (built from
    /// [`SkillIndex::names_snapshot`]).
    name: String,
}

pub struct SkillToolHandler {
    index: SkillIndex,
    project_root: PathBuf,
    /// Shared with `UserSkillsPlugin`'s own `trust_levels` (and, in turn,
    /// `App::trust_levels`), so a trust decision made via `/skills <name>`
    /// is also honored by the model calling this tool directly, and vice
    /// versa — they share one source of truth rather than each deriving
    /// its own view of trust state.
    trust_levels: TrustMap,
}

impl SkillToolHandler {
    /// Build a handler over an existing [`SkillIndex`]. The index is
    /// cheap to clone (it wraps an `Arc<RwLock<_>>`).
    pub fn new(index: SkillIndex, project_root: PathBuf, trust_levels: TrustMap) -> Self {
        Self {
            index,
            project_root,
            trust_levels,
        }
    }
}

#[async_trait]
impl InProcessToolHandler for SkillToolHandler {
    async fn call(
        &self,
        input: Value,
        _ctx: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Value, String> {
        let input: SkillInput =
            serde_json::from_value(input).map_err(|e| format!("skill: invalid input: {e}"))?;

        // Guard is dropped inside `get`; only the Arc crosses back out.
        let Some(spec) = self.index.get(&input.name).await else {
            let known = self.index.names_snapshot().await;
            return Err(unknown_skill_message(&input.name, &known));
        };

        match trust::evaluate(&spec, &self.project_root, &self.trust_levels).await {
            SkillTrust::Allowed => {}
            SkillTrust::NeedsConsent { project } => {
                return Err(trust::refusal_message(&spec.name, &project));
            }
        }

        Ok(Value::String(render_skill(
            &spec.name,
            &spec.root.display().to_string(),
            &spec.body,
        )))
    }
}

/// The level-2 payload. The root is stated explicitly because the body
/// was authored against its own directory — without it, a relative
/// `scripts/build.sh` in the instructions is unresolvable.
///
/// `pub(crate)` so `mod.rs`'s `/skills <name>` handler can reuse the
/// exact same rendering the `skill` tool returns to the model, rather
/// than duplicating the format string.
pub(crate) fn render_skill(name: &str, root: &str, body: &str) -> String {
    format!(
        "# Skill: {name}\n\n\
         Skill directory: {root}\n\
         Files referenced relatively in these instructions live under that directory \
         and can be read with `tool-fs:read_file`.\n\n\
         ---\n\n{body}"
    )
}

/// Error text for an unresolvable name. Names near-misses rather than a
/// bare "not found" — the model picked from an enum, so a miss usually
/// means the index changed under it (a `/reload-skills`, or a skill
/// shadowed by a higher tier).
///
/// `pub(crate)` so `mod.rs`'s `/skills <unknown>` path shares the same
/// near-match wording as the `skill` tool's own error rather than
/// duplicating `nearest`/`shared_prefix` in a second location.
pub(crate) fn unknown_skill_message(requested: &str, known: &[String]) -> String {
    let near = nearest(requested, known);
    if near.is_empty() {
        format!("unknown skill `{requested}`; no skills are currently loaded")
    } else {
        format!(
            "unknown skill `{requested}`; did you mean: {}",
            near.join(", ")
        )
    }
}

/// Cheap near-match: shared prefix of 3+ chars, or substring either
/// way. Enough to catch a stale or truncated name without pulling in an
/// edit-distance dependency.
fn nearest(requested: &str, known: &[String]) -> Vec<String> {
    let req = requested.to_ascii_lowercase();
    let mut hits: Vec<String> = known
        .iter()
        .filter(|k| {
            let k = k.to_ascii_lowercase();
            k.contains(&req) || req.contains(&k) || shared_prefix(&k, &req) >= 3
        })
        .cloned()
        .collect();
    hits.sort();
    hits.truncate(5);
    hits
}

fn shared_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Build the `ToolDef` exposed to the model. The `name` enum is a live
/// snapshot of discovered skills, so a call naming a stale skill fails
/// schema validation before reaching the handler.
pub async fn build_tool_def(index: &SkillIndex) -> ToolDef {
    let names = index.names_snapshot().await;
    ToolDef {
        name: "skill".into(),
        description: "Load a skill's full instructions by name. Call this when a skill listed \
                      in the available-skills catalog matches the task at hand, and follow the \
                      instructions it returns."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string", "enum": names }
            }
        }),
    }
}

/// Convenience: wrap [`SkillToolHandler`] in the newtype `Effect`
/// carries.
pub fn handler_arc(
    index: SkillIndex,
    project_root: PathBuf,
    trust_levels: TrustMap,
) -> InProcessToolHandlerArc {
    InProcessToolHandlerArc::new(SkillToolHandler::new(index, project_root, trust_levels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::user_skills::spec::{SkillScope, SkillSpec, ToolScope};
    use crate::plugin::builtin::user_slash_commands::trust::TrustLevel;
    use std::collections::BTreeMap;
    use std::path::Path;
    use tokio::sync::RwLock;

    fn spec(name: &str, scope: SkillScope, bundled: bool) -> SkillSpec {
        SkillSpec {
            name: name.into(),
            description: format!("does {name}"),
            allowed_tools: ToolScope::Inherit,
            root: PathBuf::from("/skills").join(name),
            source: PathBuf::from("/skills").join(name).join("SKILL.md"),
            scope,
            body: format!("instructions for {name}"),
            has_bundled_executables: bundled,
        }
    }

    /// The opaque context the trait requires. The skill handler ignores
    /// it, so any `Any` value will do.
    fn ctx() -> Arc<dyn std::any::Any + Send + Sync> {
        Arc::new(())
    }

    /// An empty shared trust map — no project has been decided yet.
    /// Mirrors `user_slash_commands::mod.rs` tests' own `empty_trust()`.
    fn empty_trust() -> TrustMap {
        Arc::new(RwLock::new(BTreeMap::new()))
    }

    /// A shared trust map pre-populated with one decision, the way a
    /// real session's map looks right after `Effect::SetTrustLevel`'s
    /// handler runs — not via a disk write.
    fn trust_map_with(project: &Path, level: TrustLevel) -> TrustMap {
        Arc::new(RwLock::new(BTreeMap::from([(
            project.to_path_buf(),
            level,
        )])))
    }

    #[tokio::test]
    async fn tool_def_exposes_a_name_enum_of_discovered_skills() {
        let index = SkillIndex::empty();
        index
            .replace(vec![
                spec("alpha", SkillScope::ProjectOtto, false),
                spec("beta", SkillScope::UserClaude, false),
            ])
            .await;

        let def = build_tool_def(&index).await;
        assert_eq!(def.name, "skill");
        assert_eq!(def.input_schema["required"][0], "name");
        let enumerated = def.input_schema["properties"]["name"]["enum"]
            .as_array()
            .expect("enum")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(enumerated, vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn known_skill_returns_body_and_root() {
        let index = SkillIndex::empty();
        index
            .replace(vec![spec("alpha", SkillScope::UserOtto, false)])
            .await;
        let h = SkillToolHandler::new(index, PathBuf::from("/project"), empty_trust());

        let out = h
            .call(serde_json::json!({ "name": "alpha" }), ctx())
            .await
            .expect("call");
        let text = out.as_str().expect("string result");
        assert!(
            text.contains("instructions for alpha"),
            "body must be returned"
        );
        assert!(text.contains("/skills/alpha"), "root must be stated");
        assert!(
            text.contains("tool-fs:read_file"),
            "must point at level-3 reads"
        );
    }

    #[tokio::test]
    async fn unknown_skill_names_near_matches() {
        let index = SkillIndex::empty();
        index
            .replace(vec![
                spec("rust-engineer", SkillScope::UserOtto, false),
                spec("tui-engineer", SkillScope::UserOtto, false),
            ])
            .await;
        let h = SkillToolHandler::new(index, PathBuf::from("/p"), empty_trust());

        let err = h
            .call(serde_json::json!({ "name": "rust-enginer" }), ctx())
            .await
            .unwrap_err();
        assert!(err.contains("unknown skill"), "got: {err}");
        assert!(err.contains("rust-engineer"), "got: {err}");
    }

    #[tokio::test]
    async fn unknown_skill_with_empty_index_says_so() {
        let h = SkillToolHandler::new(SkillIndex::empty(), PathBuf::from("/p"), empty_trust());
        let err = h
            .call(serde_json::json!({ "name": "whatever" }), ctx())
            .await
            .unwrap_err();
        assert!(err.contains("no skills are currently loaded"), "got: {err}");
    }

    #[tokio::test]
    async fn malformed_input_is_rejected() {
        let h = SkillToolHandler::new(SkillIndex::empty(), PathBuf::from("/p"), empty_trust());
        let err = h.call(serde_json::json!({}), ctx()).await.unwrap_err();
        assert!(err.contains("invalid input"), "got: {err}");
    }

    #[tokio::test]
    async fn untrusted_project_skill_with_scripts_is_refused() {
        let index = SkillIndex::empty();
        index
            .replace(vec![spec("deployer", SkillScope::ProjectOtto, true)])
            .await;
        let h = SkillToolHandler::new(index, PathBuf::from("/repo"), empty_trust());

        let err = h
            .call(serde_json::json!({ "name": "deployer" }), ctx())
            .await
            .unwrap_err();
        assert!(err.contains("not been trusted"), "got: {err}");
        assert!(
            !err.contains("instructions for deployer"),
            "the body must not leak in the refusal"
        );
    }

    #[tokio::test]
    async fn trusted_project_skill_with_scripts_loads() {
        let project = PathBuf::from("/repo");
        let trust = trust_map_with(&project, TrustLevel::Always);

        let index = SkillIndex::empty();
        index
            .replace(vec![spec("deployer", SkillScope::ProjectOtto, true)])
            .await;
        let h = SkillToolHandler::new(index, project, trust);

        let out = h
            .call(serde_json::json!({ "name": "deployer" }), ctx())
            .await
            .expect("call");
        assert!(out.as_str().unwrap().contains("instructions for deployer"));
    }

    /// Regression test: a `TrustLevel::SessionTextOnly` decision — made in
    /// the shared map exactly as `Effect::SetTrustLevel`'s handler would,
    /// never written to disk — must be honored by the `skill` tool
    /// immediately, the same as `TrustLevel::Always`. This is the shared
    /// `TrustMap` doing its job: a decision made via `/skills <name>`
    /// (which shares this same map) is honored here too.
    #[tokio::test]
    async fn session_text_only_project_skill_with_scripts_loads() {
        let project = PathBuf::from("/repo");
        let trust = trust_map_with(&project, TrustLevel::SessionTextOnly);

        let index = SkillIndex::empty();
        index
            .replace(vec![spec("deployer", SkillScope::ProjectOtto, true)])
            .await;
        let h = SkillToolHandler::new(index, project, trust);

        let out = h
            .call(serde_json::json!({ "name": "deployer" }), ctx())
            .await
            .expect("call");
        assert!(out.as_str().unwrap().contains("instructions for deployer"));
    }
}
