//! Repo-skill discovery for committed `SKILL.md` files.

use std::fs;
use std::path::{Path, PathBuf};

use crate::plugin::builtin::user_skills::frontmatter::parse;
use crate::plugin::builtin::user_skills::spec::SkillLocation;
use crate::plugin::builtin::user_skills::spec::SkillSpec;

pub fn discover(project_root: &Path) -> Vec<SkillSpec> {
    let mut skills = Vec::new();

    for (location, relative_root) in [
        (SkillLocation::GitHub, ".github/skills"),
        (SkillLocation::Claude, ".claude/skills"),
    ] {
        let root = project_root.join(relative_root);
        for path in skill_file_paths(&root) {
            match load_skill(&path, location) {
                Ok(skill) => skills.push(skill),
                Err(error) => {
                    tracing::warn!(path = %path.display(), "skill skipped: {error}");
                }
            }
        }
    }

    skills.sort_by(|left, right| {
        left.location
            .cmp(&right.location)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
    skills
}

fn skill_file_paths(root: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(root = %root.display(), "failed to read skill root: {error}");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            tracing::warn!(root = %root.display(), "failed to read skill directory entry");
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            tracing::warn!(path = %entry.path().display(), "failed to read skill entry type");
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let skill_path = entry.path().join("SKILL.md");
        if skill_path.is_file() {
            out.push(skill_path);
        }
    }
    out.sort();
    out
}

fn load_skill(path: &Path, location: SkillLocation) -> Result<SkillSpec, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let Some(directory) = path.parent() else {
        return Err("skill file has no parent directory".into());
    };
    let Some(directory_slug) = directory.file_name().and_then(|name| name.to_str()) else {
        return Err("skill directory slug is not valid UTF-8".into());
    };

    let parsed = parse(&raw, directory_slug, location, path)?;
    for warning in &parsed.warnings {
        tracing::warn!(path = %path.display(), "{warning}");
    }
    Ok(parsed.spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::user_skills::spec::SkillLocation;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_skill(project_root: &Path, location_dir: &str, slug: &str, body: &str) {
        let dir = project_root.join(location_dir).join(slug);
        fs::create_dir_all(&dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), body).expect("write skill");
    }

    #[test]
    fn discovers_both_locations_and_sorts_deterministically() {
        let project = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".claude/skills",
            "zeta",
            "---\nname: zeta\ndescription: Claude zeta\n---\nBody",
        );
        write_skill(
            project.path(),
            ".github/skills",
            "alpha",
            "---\nname: alpha\ndescription: GitHub alpha\n---\nBody",
        );
        write_skill(
            project.path(),
            ".github/skills",
            "beta",
            "---\nname: beta\ndescription: GitHub beta\n---\nBody",
        );

        let skills = discover(project.path());
        let got: Vec<_> = skills
            .iter()
            .map(|skill| (skill.name.as_str(), skill.location))
            .collect();

        assert_eq!(
            got,
            vec![
                ("alpha", SkillLocation::GitHub),
                ("beta", SkillLocation::GitHub),
                ("zeta", SkillLocation::Claude),
            ]
        );
    }

    #[test]
    fn skips_malformed_files_keeps_duplicates_and_ignores_non_skill_siblings() {
        let project = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".github/skills",
            "shared",
            "---\nname: shared\ndescription: GitHub shared\n---\nBody",
        );
        write_skill(
            project.path(),
            ".claude/skills",
            "shared",
            "---\nname: shared\ndescription: Claude shared\n---\nBody",
        );
        write_skill(project.path(), ".github/skills", "bad-skill", "not yaml");
        let sibling = project
            .path()
            .join(".github/skills/shared")
            .join("notes.md");
        fs::write(sibling, "ignore me").expect("write sibling");

        let skills = discover(project.path());

        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "shared");
        assert_eq!(skills[1].name, "shared");
        assert_ne!(skills[0].location, skills[1].location);
        assert!(skills.iter().all(|skill| skill.path.ends_with("SKILL.md")));
    }

    #[test]
    fn mismatch_falls_back_to_directory_slug() {
        let project = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".claude/skills",
            "rust-engineer",
            "---\nname: not-the-dir\ndescription: Claude shared\n---\nBody",
        );

        let skills = discover(project.path());

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "rust-engineer");
    }

    #[test]
    fn missing_name_is_skipped() {
        let project = tempdir().expect("tempdir");
        write_skill(
            project.path(),
            ".claude/skills",
            "rust-engineer",
            "---\ndescription: Claude shared\n---\nBody",
        );

        let skills = discover(project.path());

        assert!(skills.is_empty());
    }
}
