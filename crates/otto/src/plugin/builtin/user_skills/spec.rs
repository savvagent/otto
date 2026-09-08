//! Parsed representation of one repo-authored skill definition.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillLocation {
    GitHub,
    Claude,
}

impl SkillLocation {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::GitHub => ".github/skills",
            Self::Claude => ".claude/skills",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub location: SkillLocation,
    pub path: PathBuf,
}
