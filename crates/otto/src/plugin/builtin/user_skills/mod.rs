//! `internal:user-skills` — discovers Claude-Code-compatible skills from
//! `.otto/skills/` and `.claude/skills/` and exposes them to the model
//! under progressive disclosure, plus to the user via `/skills`.
//!
//! Three levels, matching the Claude Code format:
//!
//! 1. **Level 1** — name + description for every skill, rendered by
//!    [`index::SkillIndex::catalog`] into a single system-prompt segment.
//! 2. **Level 2** — the full `SKILL.md` body, returned as a tool result
//!    on invocation. Bodies never enter the system prompt.
//! 3. **Level 3** — bundled `scripts/` / `references/` / `assets/`, read
//!    on demand through `tool-fs` / `tool-bash` from the skill root.
//!
//! See `docs/superpowers/specs/2026-09-09-issue-83-claude-code-compat-design.md`.
//!
//! Discovery walks the four tiers the spec defines — project beats user,
//! `.otto/` beats `.claude/` — plus `<project>/.github/skills/`, which
//! Copilot CLI defines and which repos predating otto (this one included)
//! already use. `.github/` ranks last among the project tiers and has no
//! user-scope counterpart, since Copilot CLI only defines it in-repo.
//!
//! For the level-3 trust gate `.github/` counts as project scope like the
//! other two: its skills arrive with the checkout, which is the exposure
//! the gate exists for.

pub mod discovery;
pub mod frontmatter;
pub mod spec;

pub mod index;
pub mod skill_tool;
pub mod trust;

use std::path::PathBuf;

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, HookKind, HostEvent, Manifest, Plugin, PluginError, PluginId,
    PluginKind, SlashSpec, StyledLine,
};

use crate::plugin::builtin::user_skills::discovery::discover;

pub use index::SkillIndex;
#[allow(unused_imports)] // Consumed by the picker in Task 3.
pub use spec::{SkillScope, SkillSpec, ToolScope};

pub struct UserSkillsPlugin {
    project_root: PathBuf,
    user_home: PathBuf,
    index: SkillIndex,
}

impl UserSkillsPlugin {
    pub fn new() -> Self {
        let project_root = std::env::current_dir().unwrap_or_else(|error| {
            tracing::warn!("user-skills: failed to resolve current directory: {error}");
            PathBuf::from(".")
        });
        let user_home = dirs::home_dir().unwrap_or_else(|| {
            tracing::warn!("user-skills: failed to resolve home directory");
            PathBuf::from(".")
        });
        Self::with_roots(project_root, user_home)
    }

    /// Both roots are explicit so tests can point the user tiers at a
    /// tempdir instead of the developer's real `~/.claude/skills/`.
    pub fn with_roots(project_root: PathBuf, user_home: PathBuf) -> Self {
        Self {
            project_root,
            user_home,
            index: SkillIndex::empty(),
        }
    }

    /// Build the `skill` tool's registration effect, or none while no
    /// skill has been discovered yet — mirrors
    /// `UserAgentsPlugin::register_task_tool_effects`.
    async fn register_skill_tool_effects(&self) -> Vec<Effect> {
        if self.index.is_empty().await {
            return vec![];
        }
        let spec = skill_tool::build_tool_def(&self.index).await;
        let handler = skill_tool::handler_arc(
            self.index.clone(),
            self.project_root.clone(),
            self.user_home.clone(),
        );
        vec![Effect::RegisterInProcessTool { spec, handler }]
    }
}

impl Default for UserSkillsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

fn note_line(text: impl Into<String>) -> Effect {
    Effect::PushNote {
        line: StyledLine::plain(text.into()),
    }
}

#[async_trait]
impl Plugin for UserSkillsPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.slash_commands = vec![SlashSpec {
            name: "skills".into(),
            summary: "List discovered skills".into(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];
        contributions.hooks = vec![HookKind::HostStarting];
        Manifest {
            id: PluginId::new("internal:user-skills").expect("valid built-in id"),
            name: "User skills".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Skills from .otto/skills/ and .claude/skills/".into(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    async fn handle_slash(
        &mut self,
        name: &str,
        _args: Vec<String>,
    ) -> Result<Vec<Effect>, PluginError> {
        if name != "skills" {
            return Ok(vec![]);
        }

        let result = discover(&self.project_root, &self.user_home);
        let warning_note = if result.warnings.is_empty() {
            None
        } else {
            Some(note_line(format!(
                "warning: skipped {} invalid skill definition(s); see logs for details",
                result.warnings.len()
            )))
        };

        if result.skills.is_empty() {
            let mut effects = vec![note_line("no skills discovered")];
            if let Some(warning) = warning_note {
                effects.push(warning);
            }
            return Ok(effects);
        }

        let mut effects = Vec::with_capacity(result.skills.len() + 2);
        effects.push(note_line(format!(
            "skills: {} discovered (descriptions are untrusted repo text)",
            result.skills.len()
        )));
        // Descriptions come from files otto did not author, so they stay
        // labelled as untrusted wherever they are rendered.
        for skill in result.skills {
            effects.push(note_line(format!(
                "- {} [{}] — [untrusted repo skill description] {}",
                skill.name,
                skill.scope.label(),
                skill.description
            )));
        }
        if let Some(warning) = warning_note {
            effects.push(warning);
        }
        Ok(effects)
    }

    async fn on_event(&mut self, event: HostEvent) -> Result<Vec<Effect>, PluginError> {
        if matches!(event, HostEvent::HostStarting) {
            let discovered = discover(&self.project_root, &self.user_home);
            if !discovered.warnings.is_empty() {
                tracing::warn!(
                    "user-skills: skipped {} invalid skill definition(s) during startup discovery: {:?}",
                    discovered.warnings.len(),
                    discovered.warnings
                );
            }
            self.index.replace(discovered.skills).await;
            return Ok(self.register_skill_tool_effects().await);
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifests::Indexes;
    use crate::plugin::registry::PluginRegistry;
    use crate::plugin::slash::SlashRouter;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    fn write_skill(root: &std::path::Path, location_dir: &str, slug: &str, body: &str) {
        let dir = root.join(location_dir).join(slug);
        fs::create_dir_all(&dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), body).expect("write skill");
    }

    fn note_text(effect: &Effect) -> String {
        match effect {
            Effect::PushNote { line } => line.spans.iter().map(|span| span.text.clone()).collect(),
            other => panic!("expected PushNote, got {other:?}"),
        }
    }

    #[test]
    fn manifest_contributes_skills_slash() {
        let plugin = UserSkillsPlugin::default();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "internal:user-skills");
        assert!(
            manifest
                .contributions
                .slash_commands
                .iter()
                .any(|slash| slash.name == "skills")
        );
    }

    #[test]
    fn manifest_subscribes_to_host_starting() {
        let plugin = UserSkillsPlugin::default();
        let manifest = plugin.manifest();
        assert!(
            manifest
                .contributions
                .hooks
                .contains(&HookKind::HostStarting)
        );
    }

    #[tokio::test]
    async fn handle_slash_empty_state_reports_no_skills_discovered() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        let mut plugin =
            UserSkillsPlugin::with_roots(project.path().to_path_buf(), home.path().to_path_buf());

        let effects = plugin.handle_slash("skills", vec![]).await.expect("slash");

        assert_eq!(effects.len(), 1);
        assert_eq!(note_text(&effects[0]), "no skills discovered");
    }

    #[tokio::test]
    async fn invalid_skills_surface_a_warning_note() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        // Missing `description:` — the one frontmatter field that is a hard
        // parse error, because it is all the model sees at level 1.
        write_skill(
            project.path(),
            ".otto/skills",
            "broken",
            "---\nname: broken\n---\nBody",
        );
        let mut plugin =
            UserSkillsPlugin::with_roots(project.path().to_path_buf(), home.path().to_path_buf());

        let effects = plugin.handle_slash("skills", vec![]).await.expect("slash");

        let lines: Vec<_> = effects.iter().map(note_text).collect();
        assert_eq!(lines[0], "no skills discovered");
        assert_eq!(
            lines[1],
            "warning: skipped 1 invalid skill definition(s); see logs for details"
        );
    }

    #[tokio::test]
    async fn slash_output_includes_name_scope_and_description() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".otto/skills",
            "otto-development",
            "---\nname: otto-development\ndescription: Build Otto changes\n---\nBody",
        );
        write_skill(
            project.path(),
            ".claude/skills",
            "rust-engineer",
            "---\nname: rust-engineer\ndescription: Build Rust systems\n---\nBody",
        );

        let registry = PluginRegistry::from_plugins(vec![Box::new(UserSkillsPlugin::with_roots(
            project.path().to_path_buf(),
            home.path().to_path_buf(),
        ))]);
        let indexes = Indexes::build(&registry).await.expect("indexes");
        let router = SlashRouter::new(
            Arc::new(RwLock::new(indexes)),
            Arc::new(RwLock::new(registry)),
        );

        let effects = router.dispatch("skills", vec![]).await.expect("dispatch");
        let lines: Vec<_> = effects.iter().map(note_text).collect();

        assert_eq!(
            lines[0],
            "skills: 2 discovered (descriptions are untrusted repo text)"
        );
        assert!(lines[1].contains("otto-development"));
        assert!(lines[1].contains("[project/.otto]"));
        assert!(lines[1].contains("[untrusted repo skill description]"));
        assert!(lines[1].contains("Build Otto changes"));
        assert!(lines[2].contains("rust-engineer"));
        assert!(lines[2].contains("[project/.claude]"));
        assert!(lines[2].contains("[untrusted repo skill description]"));
        assert!(lines[2].contains("Build Rust systems"));
    }

    /// The four tiers share a slug namespace: the highest-precedence copy
    /// wins and the loser is reported, so an author editing the shadowed
    /// file learns why nothing changed.
    #[tokio::test]
    async fn project_tier_shadows_user_tier_and_warns() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".otto/skills",
            "shared",
            "---\nname: shared\ndescription: Project copy\n---\nBody",
        );
        write_skill(
            home.path(),
            ".claude/skills",
            "shared",
            "---\nname: shared\ndescription: User copy\n---\nBody",
        );

        let mut plugin =
            UserSkillsPlugin::with_roots(project.path().to_path_buf(), home.path().to_path_buf());
        let effects = plugin.handle_slash("skills", vec![]).await.expect("slash");
        let lines: Vec<_> = effects.iter().map(note_text).collect();

        assert_eq!(
            lines[0],
            "skills: 1 discovered (descriptions are untrusted repo text)"
        );
        assert!(lines[1].contains("Project copy"));
        assert_eq!(
            lines[2],
            "warning: skipped 1 invalid skill definition(s); see logs for details"
        );
    }

    #[tokio::test]
    async fn skill_tool_not_registered_when_index_empty() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        let plugin =
            UserSkillsPlugin::with_roots(project.path().to_path_buf(), home.path().to_path_buf());

        let effects = plugin.register_skill_tool_effects().await;

        assert!(
            effects.is_empty(),
            "no skill tool until at least one skill discovered"
        );
    }

    #[tokio::test]
    async fn skill_tool_registered_when_index_has_skills() {
        let project = tempdir().expect("tempdir");
        let home = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".otto/skills",
            "otto-development",
            "---\nname: otto-development\ndescription: Build Otto changes\n---\nBody",
        );
        let mut plugin =
            UserSkillsPlugin::with_roots(project.path().to_path_buf(), home.path().to_path_buf());

        let effects = plugin
            .on_event(HostEvent::HostStarting)
            .await
            .expect("on_event");

        assert_eq!(effects.len(), 1);
        assert!(
            matches!(effects[0], Effect::RegisterInProcessTool { .. }),
            "expected RegisterInProcessTool, got {:?}",
            effects[0]
        );
    }
}
