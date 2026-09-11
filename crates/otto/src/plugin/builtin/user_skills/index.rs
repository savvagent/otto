//! `SkillIndex` — shared slug → spec map, plus the level-1 catalog.
//!
//! The catalog is the only part of a skill that lives in the system
//! prompt, so it is capped on both axes. Level 2 (the body) is a tool
//! result; level 3 (bundled files) is read on demand. A user with no
//! skills gets no segment at all — see [`SkillIndex::catalog`].

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::plugin::builtin::user_skills::spec::SkillSpec;

/// Upper bound on catalog entries. A pathological skills directory must
/// not silently consume the context window.
#[allow(dead_code)] // Consumed by the level-1 system-prompt segment in Task 3.
pub const MAX_CATALOG_ENTRIES: usize = 200;

/// Upper bound on one description, in characters. Matches the ceiling
/// Claude Code applies to the same field.
#[allow(dead_code)] // Consumed by the level-1 system-prompt segment in Task 3.
pub const MAX_DESCRIPTION_CHARS: usize = 1024;

#[derive(Clone, Default)]
pub struct SkillIndex {
    inner: Arc<RwLock<HashMap<String, Arc<SkillSpec>>>>,
}

impl SkillIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    pub async fn replace(&self, skills: Vec<SkillSpec>) {
        let map: HashMap<String, Arc<SkillSpec>> = skills
            .into_iter()
            .map(|spec| (spec.name.clone(), Arc::new(spec)))
            .collect();
        *self.inner.write().await = map;
    }

    pub async fn get(&self, name: &str) -> Option<Arc<SkillSpec>> {
        self.inner.read().await.get(name).cloned()
    }

    #[allow(dead_code)] // Consumed by the /skills picker in Task 3.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    /// All specs, ordered by scope precedence then name — the order the
    /// `/skills` picker renders.
    #[allow(dead_code)] // Consumed by the /skills picker in Task 3.
    pub async fn sorted_snapshot(&self) -> Vec<Arc<SkillSpec>> {
        let mut all: Vec<Arc<SkillSpec>> = self.inner.read().await.values().cloned().collect();
        all.sort_by(|a, b| a.scope.cmp(&b.scope).then_with(|| a.name.cmp(&b.name)));
        all
    }

    pub async fn names_snapshot(&self) -> Vec<String> {
        let mut names: Vec<String> = self.inner.read().await.keys().cloned().collect();
        names.sort();
        names
    }

    /// Render the level-1 catalog, or `None` when there are no skills.
    ///
    /// `None` is load-bearing: it is what lets a user with zero skills
    /// pay zero tokens for the feature (spec acceptance criterion 3).
    /// The caller must omit the system-prompt segment entirely rather
    /// than emitting an empty one.
    #[allow(dead_code)] // Consumed by the live prompt-segment refresh in Task 3.
    pub async fn catalog(&self) -> Option<String> {
        let all = self.sorted_snapshot().await;
        if all.is_empty() {
            return None;
        }

        let truncated = all.len() > MAX_CATALOG_ENTRIES;
        if truncated {
            tracing::warn!(
                "{} skills discovered; catalog capped at {MAX_CATALOG_ENTRIES}",
                all.len()
            );
        }

        let mut s = String::from(
            "# Available skills\n\nEach entry is a set of instructions you can load on demand. \
             When one matches the task at hand, call the `skill` tool with its name to read the \
             full instructions before proceeding.\n\n",
        );
        for spec in all.iter().take(MAX_CATALOG_ENTRIES) {
            s.push_str(&format!(
                "- `{}` — {}\n",
                spec.name,
                truncate_chars(&spec.description, MAX_DESCRIPTION_CHARS)
            ));
        }
        if truncated {
            s.push_str(&format!(
                "\n({} further skills omitted; run /skills to see them all.)\n",
                all.len() - MAX_CATALOG_ENTRIES
            ));
        }
        Some(s)
    }
}

/// Truncate on a char boundary, appending an ellipsis when cut.
#[allow(dead_code)] // Consumed by `catalog` once Task 3 wires the prompt segment.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::user_skills::spec::{SkillScope, ToolScope};
    use std::path::PathBuf;

    fn skill(name: &str, scope: SkillScope) -> SkillSpec {
        SkillSpec {
            name: name.into(),
            description: format!("does {name}"),
            allowed_tools: ToolScope::Inherit,
            root: PathBuf::from("/tmp").join(name),
            source: PathBuf::from("/tmp").join(name).join("SKILL.md"),
            scope,
            body: format!("instructions for {name}"),
            has_bundled_executables: false,
        }
    }

    #[tokio::test]
    async fn replace_makes_skills_visible() {
        let index = SkillIndex::empty();
        index
            .replace(vec![
                skill("a", SkillScope::ProjectOtto),
                skill("b", SkillScope::UserClaude),
            ])
            .await;
        assert_eq!(index.len().await, 2);
        assert_eq!(index.get("a").await.expect("a").description, "does a");
    }

    #[tokio::test]
    async fn empty_index_renders_no_catalog() {
        let index = SkillIndex::empty();
        assert!(index.is_empty().await);
        assert!(
            index.catalog().await.is_none(),
            "zero skills must cost zero tokens"
        );
    }

    #[tokio::test]
    async fn catalog_lists_every_skill() {
        let index = SkillIndex::empty();
        index
            .replace(vec![
                skill("alpha", SkillScope::ProjectOtto),
                skill("beta", SkillScope::UserClaude),
            ])
            .await;
        let cat = index.catalog().await.expect("catalog");
        assert!(cat.contains("`alpha` — does alpha"));
        assert!(cat.contains("`beta` — does beta"));
        assert!(
            cat.contains("skill"),
            "catalog must tell the model how to load one"
        );
    }

    #[tokio::test]
    async fn catalog_orders_by_scope_then_name() {
        let index = SkillIndex::empty();
        index
            .replace(vec![
                skill("zzz", SkillScope::ProjectOtto),
                skill("aaa", SkillScope::UserClaude),
                skill("mmm", SkillScope::ProjectOtto),
            ])
            .await;
        let cat = index.catalog().await.expect("catalog");
        let mmm = cat.find("mmm").unwrap();
        let zzz = cat.find("zzz").unwrap();
        let aaa = cat.find("aaa").unwrap();
        assert!(mmm < zzz, "same scope sorts by name");
        assert!(zzz < aaa, "project scope precedes user scope");
    }

    #[tokio::test]
    async fn long_descriptions_are_truncated() {
        let index = SkillIndex::empty();
        let mut s = skill("verbose", SkillScope::ProjectOtto);
        s.description = "x".repeat(MAX_DESCRIPTION_CHARS + 500);
        index.replace(vec![s]).await;

        let cat = index.catalog().await.expect("catalog");
        assert!(cat.contains('…'), "truncation must be visible");
        let line = cat.lines().find(|l| l.contains("verbose")).unwrap();
        assert!(
            line.chars().count() <= MAX_DESCRIPTION_CHARS + 40,
            "line was {} chars",
            line.chars().count()
        );
    }

    #[tokio::test]
    async fn catalog_caps_entry_count_and_says_so() {
        let index = SkillIndex::empty();
        let many: Vec<SkillSpec> = (0..MAX_CATALOG_ENTRIES + 5)
            .map(|i| skill(&format!("s{i:04}"), SkillScope::ProjectOtto))
            .collect();
        index.replace(many).await;

        let cat = index.catalog().await.expect("catalog");
        let entries = cat.lines().filter(|l| l.starts_with("- `")).count();
        assert_eq!(entries, MAX_CATALOG_ENTRIES);
        assert!(
            cat.contains("further skills omitted"),
            "truncation must be disclosed"
        );
    }

    #[tokio::test]
    async fn truncate_respects_char_boundaries() {
        // Multi-byte input must not panic or split a char.
        let long = "é".repeat(MAX_DESCRIPTION_CHARS + 10);
        let out = truncate_chars(&long, MAX_DESCRIPTION_CHARS);
        assert!(out.chars().count() <= MAX_DESCRIPTION_CHARS);
    }
}
