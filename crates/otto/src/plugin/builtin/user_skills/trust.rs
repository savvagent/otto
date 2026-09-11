//! Level-3 trust gate for skills.
//!
//! Levels 1 and 2 need no gate: a catalog entry and a `SKILL.md` body
//! are inert markdown, and loading them executes nothing. Level 3 is
//! different — a project-local skill that ships `scripts/setup.sh` is
//! asking the model to run code that arrived with the repository. That
//! is the same exposure sub-project A's `!<cmd>` commands carry, so it
//! reuses A's decision store — not the on-disk file directly, but the
//! same live, shared, in-memory `TrustMap` that
//! `internal:user-slash-commands` reads and writes (see that plugin's
//! `mod.rs`), which is what the disk file (`~/.otto/trusted-projects.json`)
//! seeds at startup and `TrustLevel::Always` decisions are persisted back
//! to. Reading the shared map instead of re-reading the disk file on every
//! call is what lets a `TrustLevel::SessionTextOnly` decision (deliberately
//! never persisted — see `user_slash_commands::trust`'s own doc comment)
//! be honored on the very next `/skills <name>` or `skill` tool call in the
//! same session, rather than re-opening the trust modal every time.
//!
//! Enforcement point is the *body*, not the scripts. Withholding
//! instructions that tell the model to run bundled code is what
//! actually prevents the run; the files themselves are already
//! reachable by any tool with filesystem access.

use std::path::Path;

use crate::plugin::builtin::user_skills::spec::{SkillScope, SkillSpec};
use crate::plugin::builtin::user_slash_commands::TrustMap;
use crate::plugin::builtin::user_slash_commands::trust::TrustLevel;

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
///
/// `trust_levels` is the same shared, live `TrustMap`
/// `internal:user-slash-commands` reads and writes — seeded from
/// `~/.otto/trusted-projects.json` at startup and kept current by
/// `Effect::SetTrustLevel`'s handler for both `TrustLevel::Always`
/// (persisted) and `TrustLevel::SessionTextOnly` (session-only). Reading
/// it directly, instead of re-reading the disk file on every call, is
/// what lets a session-text-only decision be honored immediately rather
/// than only on the next process start.
pub async fn evaluate(
    spec: &SkillSpec,
    project_root: &Path,
    trust_levels: &TrustMap,
) -> SkillTrust {
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

    let map = trust_levels.read().await;
    match map.get(project_root) {
        Some(TrustLevel::Always) | Some(TrustLevel::SessionTextOnly) => SkillTrust::Allowed,
        None => SkillTrust::NeedsConsent {
            project: project_root.display().to_string(),
        },
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
    use std::sync::Arc;
    use tokio::sync::RwLock;

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

    /// An empty shared trust map — no project has been decided yet.
    /// Mirrors `user_slash_commands::mod.rs` tests' own `empty_trust()`.
    fn empty_trust() -> TrustMap {
        Arc::new(RwLock::new(BTreeMap::new()))
    }

    /// A shared trust map pre-populated with one decision — the way a
    /// real session's map looks right after `Effect::SetTrustLevel`'s
    /// handler runs, not via a disk write.
    fn trust_map_with(project: &Path, level: TrustLevel) -> TrustMap {
        Arc::new(RwLock::new(BTreeMap::from([(
            project.to_path_buf(),
            level,
        )])))
    }

    #[tokio::test]
    async fn skill_without_bundled_files_is_always_allowed() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();
        for scope in [
            SkillScope::ProjectOtto,
            SkillScope::ProjectClaude,
            SkillScope::ProjectGithub,
            SkillScope::UserOtto,
            SkillScope::UserClaude,
        ] {
            assert_eq!(
                evaluate(&spec(scope, false), &project, &trust).await,
                SkillTrust::Allowed
            );
        }
    }

    /// A `.github/skills/` skill shipping runnable files is gated the
    /// same as `.otto/` and `.claude/` — it came with the repository.
    #[tokio::test]
    async fn github_scope_skill_with_scripts_needs_consent() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();
        assert_eq!(
            evaluate(&spec(SkillScope::ProjectGithub, true), &project, &trust).await,
            SkillTrust::NeedsConsent {
                project: project.display().to_string()
            }
        );
    }

    #[tokio::test]
    async fn user_scope_skill_with_scripts_never_prompts() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();
        assert_eq!(
            evaluate(&spec(SkillScope::UserOtto, true), &project, &trust).await,
            SkillTrust::Allowed
        );
        assert_eq!(
            evaluate(&spec(SkillScope::UserClaude, true), &project, &trust).await,
            SkillTrust::Allowed
        );
    }

    #[tokio::test]
    async fn untrusted_project_skill_with_scripts_needs_consent() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();
        match evaluate(&spec(SkillScope::ProjectOtto, true), &project, &trust).await {
            SkillTrust::NeedsConsent { project: p } => assert!(p.contains("some/project")),
            other => panic!("expected NeedsConsent, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn trusted_project_skill_with_scripts_is_allowed() {
        let project = PathBuf::from("/some/project");
        let trust = trust_map_with(&project, TrustLevel::Always);

        assert_eq!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &project, &trust).await,
            SkillTrust::Allowed
        );
    }

    /// Regression test for the trust-gate bug: a user picking
    /// "session-text-only" in the trust modal must be honored on the
    /// very next `evaluate()` call for the same project — without
    /// anything being written to disk. Before the fix, `evaluate` read
    /// `~/.otto/trusted-projects.json` fresh on every call, which never
    /// sees a `SessionTextOnly` decision (deliberately never persisted),
    /// so the trust modal reopened every time.
    #[tokio::test]
    async fn session_text_only_decision_is_honored_without_disk_persistence() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();

        // First call: no decision yet, gated.
        assert!(matches!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &project, &trust).await,
            SkillTrust::NeedsConsent { .. }
        ));

        // Simulate the user choosing "session-text-only" in the trust
        // modal, exactly as `Effect::SetTrustLevel`'s handler would:
        // insert into the shared map, no disk write.
        trust
            .write()
            .await
            .insert(project.clone(), TrustLevel::SessionTextOnly);

        // Second call, same session: must now be allowed.
        assert_eq!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &project, &trust).await,
            SkillTrust::Allowed
        );
    }

    #[tokio::test]
    async fn trust_is_per_project_not_global() {
        let trusted = PathBuf::from("/trusted");
        let other = PathBuf::from("/other");
        let trust = trust_map_with(&trusted, TrustLevel::Always);

        assert!(matches!(
            evaluate(&spec(SkillScope::ProjectOtto, true), &other, &trust).await,
            SkillTrust::NeedsConsent { .. }
        ));
    }

    #[tokio::test]
    async fn plugin_scope_is_gated_upstream_not_here() {
        let project = PathBuf::from("/some/project");
        let trust = empty_trust();
        assert_eq!(
            evaluate(
                &spec(SkillScope::Plugin("acme".into()), true),
                &project,
                &trust
            )
            .await,
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
