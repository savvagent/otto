//! `internal:user-skills` — discovers repo-authored skills and exposes
//! them via the `/skills` built-in slash command.

mod discovery;
mod frontmatter;
mod spec;

use std::path::PathBuf;

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, Manifest, Plugin, PluginError, PluginId, PluginKind, SlashSpec,
    StyledLine,
};

use crate::plugin::builtin::user_skills::discovery::discover;

pub struct UserSkillsPlugin {
    project_root: PathBuf,
}

impl UserSkillsPlugin {
    pub fn new() -> Self {
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self { project_root }
    }

    pub fn with_project_root(project_root: PathBuf) -> Self {
        Self { project_root }
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
            summary: "List repo-authored skills".into(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];
        Manifest {
            id: PluginId::new("internal:user-skills").expect("valid built-in id"),
            name: "User skills".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Repo-authored skills from .github/skills/ and .claude/skills/".into(),
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

        let skills = discover(&self.project_root);
        if skills.is_empty() {
            return Ok(vec![note_line("no skills discovered")]);
        }

        let mut effects = Vec::with_capacity(skills.len() + 1);
        effects.push(note_line(format!("skills: {} discovered", skills.len())));
        for skill in skills {
            effects.push(note_line(format!(
                "- {} [{}] — {}",
                skill.name,
                skill.location.as_label(),
                skill.description
            )));
        }
        Ok(effects)
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

    fn write_skill(project_root: &std::path::Path, location_dir: &str, slug: &str, body: &str) {
        let dir = project_root.join(location_dir).join(slug);
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

    #[tokio::test]
    async fn handle_slash_empty_state_reports_no_skills_discovered() {
        let project = tempdir().expect("tempdir");
        let mut plugin = UserSkillsPlugin::with_project_root(project.path().to_path_buf());

        let effects = plugin.handle_slash("skills", vec![]).await.expect("slash");

        assert_eq!(effects.len(), 1);
        assert_eq!(note_text(&effects[0]), "no skills discovered");
    }

    #[tokio::test]
    async fn slash_output_includes_name_location_and_description() {
        let project = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".github/skills",
            "otto-development",
            "---\nname: otto-development\ndescription: Build Otto changes\n---\nBody",
        );
        write_skill(
            project.path(),
            ".claude/skills",
            "rust-engineer",
            "---\nname: rust-engineer\ndescription: Build Rust systems\n---\nBody",
        );

        let registry = PluginRegistry::from_plugins(vec![Box::new(
            UserSkillsPlugin::with_project_root(project.path().to_path_buf()),
        )]);
        let indexes = Indexes::build(&registry).await.expect("indexes");
        let router = SlashRouter::new(
            Arc::new(RwLock::new(indexes)),
            Arc::new(RwLock::new(registry)),
        );

        let effects = router.dispatch("skills", vec![]).await.expect("dispatch");
        let lines: Vec<_> = effects.iter().map(note_text).collect();

        assert_eq!(lines[0], "skills: 2 discovered");
        assert!(lines[1].contains("otto-development"));
        assert!(lines[1].contains("[.github/skills]"));
        assert!(lines[1].contains("Build Otto changes"));
        assert!(lines[2].contains("rust-engineer"));
        assert!(lines[2].contains("[.claude/skills]"));
        assert!(lines[2].contains("Build Rust systems"));
    }
}
