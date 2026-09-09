//! `SkillSpec` — parsed representation of one `SKILL.md` definition.
//!
//! A skill is a *directory* (unlike an agent, which is a single `.md`
//! file): the slug comes from the directory name, `SKILL.md` holds the
//! frontmatter and instructions, and any sibling files are level-3
//! resources the model reads on demand via `tool-fs` / `tool-bash`.
//! See `docs/superpowers/specs/2026-09-09-issue-83-claude-code-compat-design.md`.

use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SkillSpec {
    /// Index key. Always the directory slug, even when frontmatter
    /// `name` disagrees (the disagreement warns; the slug wins).
    pub name: String,
    /// Level-1 text. The only field the model sees before invocation,
    /// which is why a missing one is a hard parse error.
    pub description: String,
    /// Advisory in E1; enforced in E4 via `ScopedToolRegistry`.
    // Read by `trust`/`skill_tool`, which stay unwired until Task 2.
    #[allow(dead_code)]
    pub allowed_tools: ToolScope,
    /// The skill directory. Handed to the model on invocation so
    /// relative references in the body resolve for level-3 reads.
    pub root: PathBuf,
    /// The `SKILL.md` file itself, for diagnostics and the picker.
    // Read by `trust`/`skill_tool`, which stay unwired until Task 2.
    #[allow(dead_code)]
    pub source: PathBuf,
    pub scope: SkillScope,
    /// Level-2 text. Returned as a tool result, never placed in the
    /// system prompt.
    pub body: String,
    /// Whether the directory carries anything runnable. Drives the
    /// level-3 project-trust prompt in Task 2; recorded here so the
    /// directory is walked once, at discovery.
    pub has_bundled_executables: bool,
}

/// Frontmatter `allowed-tools`. Advisory in E1; enforced in E4 via
/// `otto_host::ScopedToolRegistry`, the same mechanism backing subagent
/// `tools:` scoping.
///
/// There is deliberately no `Empty` variant. A list that filters down to
/// nothing — every entry naming a tool otto does not register — means the
/// author wrote tool names for a different agent, not that they intended
/// to deny everything. That case collapses to `Inherit` and warns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolScope {
    /// `allowed-tools:` absent, or present but naming nothing known.
    Inherit,
    /// Explicit allowlist, non-empty.
    Allowed(HashSet<String>),
}

/// Which discovery tier a skill came from. Ordered highest-precedence
/// first; `Ord` is derived from that order so callers can sort by it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillScope {
    ProjectOtto,
    ProjectClaude,
    /// `<project>/.github/skills/` — Copilot CLI's location. Not a Claude
    /// Code tier, but repos that predate otto keep their skills here, so
    /// otto reads it rather than making them move. Project-scoped, so it
    /// ranks above every user tier; below `.claude/` because that is the
    /// format otto's own docs tell authors to write.
    ///
    /// There is no user-scope equivalent: Copilot CLI defines
    /// `.github/skills/` only inside a repository.
    ProjectGithub,
    UserOtto,
    UserClaude,
    /// Contributed by a Claude Code plugin (E2). Ranks below every
    /// filesystem tier so a user can shadow a plugin skill by slug.
    // Nothing constructs this until E2 lands plugin-contributed skills;
    // the variant exists now so the precedence order is fixed once.
    #[allow(dead_code)]
    Plugin(String),
}

impl SkillScope {
    /// Short badge for the `/skills` picker and warning text.
    pub fn label(&self) -> String {
        match self {
            Self::ProjectOtto => "project/.otto".into(),
            Self::ProjectClaude => "project/.claude".into(),
            Self::ProjectGithub => "project/.github".into(),
            Self::UserOtto => "user/.otto".into(),
            Self::UserClaude => "user/.claude".into(),
            Self::Plugin(id) => format!("plugin:{id}"),
        }
    }
}
