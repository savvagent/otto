//! YAML frontmatter parser for repo-authored `SKILL.md` files.

use std::path::Path;

use serde::Deserialize;

use crate::plugin::builtin::user_skills::spec::{SkillLocation, SkillSpec};

#[derive(Debug)]
pub struct FrontmatterResult {
    pub spec: SkillSpec,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

pub fn parse(
    raw: &str,
    directory_slug: &str,
    location: SkillLocation,
    path: &Path,
) -> Result<FrontmatterResult, String> {
    let (frontmatter, _body) = split_frontmatter(raw)?;
    let raw_frontmatter: RawFrontmatter = serde_yaml_ng::from_str(frontmatter)
        .map_err(|error| format!("malformed frontmatter: {error}"))?;

    let name = raw_frontmatter
        .name
        .ok_or_else(|| "missing required field: name".to_string())?;

    let description = raw_frontmatter
        .description
        .ok_or_else(|| "missing required field: description".to_string())?;

    let mut warnings = Vec::new();
    let name = match name {
        name if name != directory_slug => {
            warnings.push(format!(
                "frontmatter name `{name}` disagrees with directory slug `{directory_slug}`; directory slug wins"
            ));
            directory_slug.to_string()
        }
        _ => directory_slug.to_string(),
    };

    Ok(FrontmatterResult {
        spec: SkillSpec {
            name,
            description,
            location,
            path: path.to_path_buf(),
        },
        warnings,
    })
}

fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    if !raw.starts_with("---") {
        return Err("no frontmatter delimiter".into());
    }
    let rest = &raw[3..];
    let rest = if let Some(stripped) = rest.strip_prefix("\r\n") {
        stripped
    } else {
        rest.strip_prefix('\n').unwrap_or(rest)
    };
    let Some((front, body)) = split_closing(rest) else {
        return Err("unterminated frontmatter".into());
    };
    Ok((front, body))
}

fn split_closing(after_open: &str) -> Option<(&str, &str)> {
    for (idx, _) in after_open.match_indices("\n---") {
        let after = &after_open[idx + 4..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
            let yaml = &after_open[..idx];
            let body_start = if let Some(stripped) = after.strip_prefix("\r\n") {
                stripped
            } else {
                after.strip_prefix('\n').unwrap_or(after)
            };
            return Some((yaml, body_start));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_name_and_description() {
        let result = parse(
            "---\nname: rust-engineer\ndescription: Rust skill\n---\nBody",
            "rust-engineer",
            SkillLocation::Claude,
            Path::new("SKILL.md"),
        )
        .expect("parse");

        assert_eq!(result.spec.name, "rust-engineer");
        assert_eq!(result.spec.description, "Rust skill");
        assert_eq!(result.spec.location, SkillLocation::Claude);
        assert_eq!(result.spec.path, PathBuf::from("SKILL.md"));
    }

    #[test]
    fn name_mismatch_falls_back_to_directory_slug() {
        let result = parse(
            "---\nname: mismatched\ndescription: Rust skill\n---\nBody",
            "rust-engineer",
            SkillLocation::GitHub,
            Path::new(".github/skills/rust-engineer/SKILL.md"),
        )
        .expect("parse");

        assert_eq!(result.spec.name, "rust-engineer");
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("mismatched"))
        );
    }

    #[test]
    fn missing_name_fails() {
        let err = parse(
            "---\ndescription: Rust skill\n---\nBody",
            "rust-engineer",
            SkillLocation::Claude,
            Path::new("SKILL.md"),
        )
        .unwrap_err();

        assert!(err.contains("missing required field: name"));
    }

    #[test]
    fn only_accepts_standalone_closing_delimiter() {
        let err = parse(
            "---\nname: rust-engineer\ndescription: |\n  --- not a delimiter\nBody",
            "rust-engineer",
            SkillLocation::Claude,
            Path::new("SKILL.md"),
        )
        .unwrap_err();

        assert!(err.contains("unterminated frontmatter"));
    }

    #[test]
    fn supports_crlf_delimiters() {
        let result = parse(
            "---\r\nname: rust-engineer\r\ndescription: Rust skill\r\n---\r\nBody",
            "rust-engineer",
            SkillLocation::Claude,
            Path::new("SKILL.md"),
        )
        .expect("parse");

        assert_eq!(result.spec.name, "rust-engineer");
        assert_eq!(result.spec.description, "Rust skill");
    }
}
