//! YAML frontmatter parser for `SKILL.md` files.
//!
//! Deliberately permissive about unknown keys: Claude Code ships skills
//! carrying `license` and `metadata` in the wild, and a hard error there
//! would reject a user's working skill for a field otto simply has no use
//! for. Unknown keys warn once and are ignored. `description` is the one
//! required field, because it is the only thing the model sees at level 1.

use std::collections::HashSet;

use serde::Deserialize;

use crate::plugin::builtin::user_skills::spec::ToolScope;

#[derive(Debug)]
pub struct FrontmatterResult {
    pub name: String,
    pub description: String,
    pub allowed_tools: ToolScope,
    pub body: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default, alias = "allowed-tools")]
    allowed_tools: Option<ToolsField>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ToolsField {
    Str(String),
    List(Vec<String>),
}

/// Keys otto reads. Anything else warns rather than failing.
const KNOWN_KEYS: &[&str] = &["name", "description", "allowed-tools", "allowed_tools"];

pub fn parse(raw: &str, dir_slug: &str) -> Result<FrontmatterResult, String> {
    let (front, body) = split_frontmatter(raw)?;

    let mut warnings = Vec::new();

    // Unknown-key detection needs the raw mapping; the typed struct
    // silently drops anything it does not name.
    if let Ok(serde_yaml_ng::Value::Mapping(map)) = serde_yaml_ng::from_str(front) {
        let mut unknown: Vec<String> = map
            .keys()
            .filter_map(|k| k.as_str())
            .filter(|k| !KNOWN_KEYS.contains(k))
            .map(|k| k.to_string())
            .collect();
        unknown.sort();
        if !unknown.is_empty() {
            warnings.push(format!(
                "ignoring unknown frontmatter keys: {}",
                unknown.join(", ")
            ));
        }
    }

    let raw_front: RawFrontmatter =
        serde_yaml_ng::from_str(front).map_err(|e| format!("malformed frontmatter: {e}"))?;

    let description = raw_front
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .ok_or_else(|| "missing required field: description".to_string())?;

    if body.trim().is_empty() {
        return Err("empty body".into());
    }

    let name = match raw_front.name {
        Some(n) if n.trim() != dir_slug => {
            warnings.push(format!(
                "frontmatter name `{}` disagrees with directory slug `{dir_slug}`; directory wins",
                n.trim()
            ));
            dir_slug.to_string()
        }
        _ => dir_slug.to_string(),
    };

    let allowed_tools = match raw_front.allowed_tools {
        None => ToolScope::Inherit,
        Some(ToolsField::List(list)) => scope_from_iter(list.into_iter(), &mut warnings),
        Some(ToolsField::Str(s)) => {
            scope_from_iter(s.split(',').map(|p| p.to_string()), &mut warnings)
        }
    };

    Ok(FrontmatterResult {
        name,
        description,
        allowed_tools,
        body: body.trim_start_matches('\n').to_string(),
        warnings,
    })
}

/// Collapse an `allowed-tools` list into a scope. An all-empty list is
/// `Inherit`, not a deny-all — see `ToolScope`'s docs.
fn scope_from_iter<I: Iterator<Item = String>>(it: I, warnings: &mut Vec<String>) -> ToolScope {
    let set: HashSet<String> = it
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if set.is_empty() {
        warnings.push("`allowed-tools` is empty; treating as unset".into());
        ToolScope::Inherit
    } else {
        ToolScope::Allowed(set)
    }
}

fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    if !raw.starts_with("---") {
        return Err("no frontmatter delimiter".into());
    }
    let rest = &raw[3..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let Some(end) = rest.find("\n---") else {
        return Err("unterminated frontmatter".into());
    };
    let front = &rest[..end];
    let after = &rest[end + 4..];
    Ok((front, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "---\nname: rust-engineer\ndescription: Use when building Rust systems.\nallowed-tools: tool-fs:read_file, tool-grep:search\n---\n\n# Rust engineer\n\ninstructions here\n";

    #[test]
    fn parses_name_description_and_tools() {
        let r = parse(FULL, "rust-engineer").expect("parse");
        assert_eq!(r.name, "rust-engineer");
        assert_eq!(r.description, "Use when building Rust systems.");
        match r.allowed_tools {
            ToolScope::Allowed(set) => {
                assert!(set.contains("tool-fs:read_file"));
                assert!(set.contains("tool-grep:search"));
                assert_eq!(set.len(), 2);
            }
            other => panic!("expected allowlist, got {other:?}"),
        }
        assert!(r.body.contains("# Rust engineer"));
        assert!(
            r.warnings.is_empty(),
            "unexpected warnings: {:?}",
            r.warnings
        );
    }

    #[test]
    fn missing_description_is_a_hard_error() {
        let raw = "---\nname: x\n---\nbody";
        let err = parse(raw, "x").unwrap_err();
        assert!(err.contains("description"), "got: {err}");
    }

    #[test]
    fn blank_description_is_a_hard_error() {
        let raw = "---\ndescription: \"   \"\n---\nbody";
        let err = parse(raw, "x").unwrap_err();
        assert!(err.contains("description"), "got: {err}");
    }

    #[test]
    fn missing_name_defaults_to_dir_slug_without_warning() {
        let raw = "---\ndescription: hi\n---\nbody";
        let r = parse(raw, "my-skill").expect("parse");
        assert_eq!(r.name, "my-skill");
        assert!(
            r.warnings.is_empty(),
            "unexpected warnings: {:?}",
            r.warnings
        );
    }

    #[test]
    fn name_mismatch_warns_but_dir_slug_wins() {
        let raw = "---\nname: something-else\ndescription: hi\n---\nbody";
        let r = parse(raw, "my-skill").expect("parse");
        assert_eq!(r.name, "my-skill");
        assert!(
            r.warnings.iter().any(|w| w.contains("disagrees")),
            "got: {:?}",
            r.warnings
        );
    }

    #[test]
    fn unknown_keys_warn_and_are_ignored() {
        let raw = "---\ndescription: hi\nlicense: MIT\nmetadata:\n  a: b\n---\nbody";
        let r = parse(raw, "x").expect("parse");
        let joined = r.warnings.join(" ");
        assert!(joined.contains("license"), "got: {:?}", r.warnings);
        assert!(joined.contains("metadata"), "got: {:?}", r.warnings);
    }

    #[test]
    fn allowed_tools_accepts_a_yaml_list() {
        let raw = "---\ndescription: hi\nallowed-tools:\n  - tool-fs:read_file\n  - tool-bash:run\n---\nbody";
        let r = parse(raw, "x").expect("parse");
        match r.allowed_tools {
            ToolScope::Allowed(set) => assert_eq!(set.len(), 2),
            other => panic!("expected allowlist, got {other:?}"),
        }
    }

    #[test]
    fn empty_allowed_tools_collapses_to_inherit_with_warning() {
        let raw = "---\ndescription: hi\nallowed-tools: \"  ,  \"\n---\nbody";
        let r = parse(raw, "x").expect("parse");
        assert_eq!(r.allowed_tools, ToolScope::Inherit);
        assert!(
            r.warnings.iter().any(|w| w.contains("empty")),
            "got: {:?}",
            r.warnings
        );
    }

    #[test]
    fn empty_body_is_a_hard_error() {
        let raw = "---\ndescription: hi\n---\n   \n";
        let err = parse(raw, "x").unwrap_err();
        assert!(err.contains("body"), "got: {err}");
    }

    #[test]
    fn missing_frontmatter_delimiter_errors() {
        let err = parse("no frontmatter here", "x").unwrap_err();
        assert!(err.contains("delimiter"), "got: {err}");
    }

    #[test]
    fn unterminated_frontmatter_errors() {
        let err = parse("---\ndescription: hi\nbody with no close", "x").unwrap_err();
        assert!(err.contains("unterminated"), "got: {err}");
    }
}
