//! Level-3 trust gate for skills.
//!
//! Levels 1 and 2 need no gate: a catalog entry and a `SKILL.md` body
//! are inert markdown, and loading them executes nothing. Level 3 is
//! different — a project-local skill that ships `scripts/setup.sh` is
//! asking the model to run code that arrived with the repository. That
//! is the same exposure sub-project A's `!<cmd>` commands carry, so it
//! reuses A's decision store (`~/.otto/trusted-projects.json`) rather
//! than introducing a second trust file with its own semantics.
//!
//! Enforcement point is the *body*, not the scripts. Withholding
//! instructions that tell the model to run bundled code is what
//! actually prevents the run; the files themselves are already
//! reachable by any tool with filesystem access.

use std::path::Path;

use crate::plugin::builtin::user_skills::spec::{SkillScope, SkillSpec};
use crate::plugin::builtin::user_slash_commands::trust::{self, TrustLevel};

/// Outcome of evaluating one skill against the persisted trust store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillTrust {
    /// Safe to hand the body over.
    Allowed,
    /// Project-local, carries runnable files, and the project has not
    /// been trusted. Carries the project path for the message shown to
    /// the user.
    NeedsConsent { project: String },
}

/// Decide whether `spec`'s body may be returned.
///
/// User-scope skills are trusted implicitly, exactly as user-scope
/// commands are: they live in the user's own home directory and did
/// not arrive with a checkout. Plugin-scope skills are gated upstream
/// by E2's SHA-256 consent flow, so they are not re-gated here.
pub fn evaluate(spec: &SkillSpec, project_root: &Path, home: &Path) -> SkillTrust {
    if !spec.has_bundled_executables {
        return SkillTrust::Allowed;
    }

    match spec.scope {
        SkillScope::UserOtto | SkillScope::UserClaude | SkillScope::Plugin(_) => {
            return SkillTrust::Allowed;
        }
        // `.github/` is project scope like the other two: the skill and
        // its bundled scripts arrived with the checkout, which is exactly
        // the exposure this gate exists for. Where the repo chose to put
        // it changes nothing about who wrote it.
        SkillScope::ProjectOtto | SkillScope::ProjectClaude | SkillScope::ProjectGithub => {}
    }

    let (levels, warning) = trust::load(home);
    if let Some(w) = warning {
        tracing::warn!("skill trust: {w}");
    }
    if matches!(levels.get(project_root), Some(TrustLevel::Always)) {
        SkillTrust::Allowed
    } else {
        SkillTrust::NeedsConsent {
            project: project_root.display().to_string(),
        }
    }
}

/// The refusal text handed back to the model when a skill is gated.
/// Phrased for the model but readable by the user, and it names the
/// remedy rather than just declining.
pub fn refusal_message(name: &str, project: &str) -> String {
    format!(
        "skill `{name}` is not available: it ships runnable files and the project `{project}` \
         has not been trusted. Do not attempt to read or run its bundled files. The user can \
         trust this project by invoking the skill from the `/skills` picker and confirming, \
         after which this skill will load normally."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::user_skills::spec::ToolScope;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn spec(scope: SkillScope, bundled: bool) -> SkillSpec {
        SkillSpec {
            name: "s".into(),
            description: "d".into(),
            allowed_tools: ToolScope::Inherit,
            root: PathBuf::from("/tmp/s"),
            source: PathBuf::from("/tmp/s/SKILL.md"),
            scope,
            body: "instructions".into(),
            has_bundled_executables: bundled,
        }
    }

    #[test]
    fn skill_without_bundled_files_is_always_allowed() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        for scope in [
            SkillScope::ProjectOtto,
            SkillScope::ProjectClaude,
            SkillScope::ProjectGithub,
            SkillScope::UserOtto,
            SkillScope::UserClaude,
        ] {
            assert_eq!(
                evaluate(&spec(scope, false), &project, home.path()),
                SkillTrust::Allowed
            );
        }
    }

    /// A `.github/skills/` skill shipping runnable files is gated the
    /// same as `.otto/` and `.claude/` — it came with the repository.
    #[test]
    fn github_scope_skill_with_scripts_needs_consent() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        assert_eq!(
            evaluate(
                &spec(SkillScope::ProjectGithub, true),
                &project,
                home.path()
            ),
            SkillTrust::NeedsConsent {
                project: project.display().to_string()
            }
        );
    }

    #[test]
    fn user_scope_skill_with_scripts_never_prompts() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        assert_eq!(
            evaluate(&spec(SkillScope::UserOtto, true), &project, home.path()),
            SkillTrust::Allowed
        );
        assert_eq!(
            evaluate(&spec(SkillScope::UserClaude, true), &project, home.path()),
            SkillTrust::Allowed
        );
    }

    #[test]
    fn untrusted_project_skill_with_scripts_needs_consent() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        match evaluate(&spec(SkillScope::ProjectOtto, true), &project, home.path()) {
            SkillTrust::NeedsConsent { project: p } => assert!(p.contains("some/project")),
            other => panic!("expected NeedsConsent, got {other:?}"),
        }
    }

    #[test]
    fn trusted_project_skill_with_scripts_is_allowed() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        let mut levels = BTreeMap::new();
        levels.insert(project.clone(), TrustLevel::Always);
        trust::save(home.path(), &levels).expect("save trust");

        assert_eq!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &project, home.path()),
            SkillTrust::Allowed
        );
    }

    #[test]
    fn trust_is_per_project_not_global() {
        let home = tempdir().unwrap();
        let trusted = PathBuf::from("/trusted");
        let other = PathBuf::from("/other");
        let mut levels = BTreeMap::new();
        levels.insert(trusted, TrustLevel::Always);
        trust::save(home.path(), &levels).expect("save trust");

        assert!(matches!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &other, home.path()),
            SkillTrust::NeedsConsent { .. }
        ));
    }

    #[test]
    fn plugin_scope_is_gated_upstream_not_here() {
        let home = tempdir().unwrap();
        let project = PathBuf::from("/some/project");
        assert_eq!(
            evaluate(
                &spec(SkillScope::Plugin("acme".into()), true),
                &project,
                home.path()
            ),
            SkillTrust::Allowed
        );
    }

    #[test]
    fn refusal_names_the_skill_the_project_and_the_remedy() {
        let msg = refusal_message("deployer", "/repo");
        assert!(msg.contains("deployer"));
        assert!(msg.contains("/repo"));
        assert!(msg.contains("/skills"), "must name the remedy");
        assert!(
            msg.contains("Do not"),
            "must tell the model not to route around it"
        );
    }
}
