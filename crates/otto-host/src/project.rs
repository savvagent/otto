//! Project-level context.
//!
//! `OTTO.md` carries optional YAML front-matter delimited by `---`
//! lines. The body (whatever follows the closing delimiter, or the whole
//! file if no front-matter is present) gets injected into the system
//! prompt. The front-matter feeds [`crate::permissions::PermissionPolicy`]
//! its project-pinned rules.

use std::path::Path;

use serde::Deserialize;

use crate::permissions::FrontMatterPermissions;

/// File the host looks for at the project root for project-specific
/// instructions (analogous to OpenCode's `AGENTS.md`).
pub const PROJECT_CONTEXT_FILE: &str = "OTTO.md";

/// Result of [`parse_otto_md`]: a possibly-empty body plus whatever
/// permissions section was parsed from the front-matter. Both fields are
/// safely defaulted on missing file or malformed front-matter — callers
/// don't need to distinguish "no front-matter" from "front-matter parse
/// error" since the policy treats both as "no overrides."
#[derive(Debug, Default, Clone)]
pub struct ParsedProjectContext {
    /// Body text below the front-matter (or the full file if there's none).
    /// Already trimmed of leading/trailing whitespace.
    pub body: Option<String>,
    /// Permissions parsed from the front-matter, or empty defaults.
    pub permissions: FrontMatterPermissions,
}

/// Read `OTTO.md` (if present) and split into body + front-matter.
/// Falls back silently to defaults on missing file, malformed
/// front-matter, or unknown YAML schema.
pub fn parse_otto_md(project_root: &Path) -> ParsedProjectContext {
    let Ok(text) = std::fs::read_to_string(project_root.join(PROJECT_CONTEXT_FILE)) else {
        return ParsedProjectContext::default();
    };
    parse_text(&text)
}

#[derive(Debug, Default, Clone, Deserialize)]
struct FrontMatterDoc {
    #[serde(default)]
    permissions: FrontMatterPermissions,
}

/// Strip an optional `---`-delimited YAML block from the head of `text`,
/// parse its `permissions` section, and return the body + permissions.
fn parse_text(text: &str) -> ParsedProjectContext {
    if let Some((yaml, body)) = split_front_matter(text) {
        let permissions = serde_yaml_ng::from_str::<FrontMatterDoc>(yaml)
            .map(|d| d.permissions)
            .unwrap_or_default();
        let body = if body.trim().is_empty() {
            None
        } else {
            Some(body.trim().to_string())
        };
        ParsedProjectContext { body, permissions }
    } else {
        let body = if text.trim().is_empty() {
            None
        } else {
            Some(text.trim().to_string())
        };
        ParsedProjectContext {
            body,
            permissions: FrontMatterPermissions::default(),
        }
    }
}

/// Match `^---\n<yaml>\n---(\n|$)<body>` and return `(yaml, body)`. Returns
/// `None` if `text` doesn't open with a `---` line or has no closing
/// delimiter on its own line.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let close_idx = rest.find("\n---\n").or_else(|| {
        // Allow a trailing `---` with no newline after it (file ends there).
        if rest.ends_with("\n---") {
            Some(rest.len() - 4)
        } else {
            None
        }
    })?;
    let yaml = &rest[..close_idx];
    // Skip the closing delimiter and one optional newline.
    let after = &rest[close_idx + 4..];
    let body = after.strip_prefix('\n').unwrap_or(after);
    Some((yaml, body))
}

/// Stitch up to three named layers (default prompt, embedder override,
/// `OTTO.md` body) into one system-prompt string. Each present
/// layer is wrapped with a Markdown H1 heading and separated by blank
/// lines. Layers that are `None`, empty after trim, or whitespace-only
/// after trim are skipped — no heading, no separator emitted for them.
/// Returns `None` only when every layer collapses to absent.
///
/// Non-empty layers are rendered as-is — `trim()` is consulted only
/// for the emptiness gate. This preserves intentional whitespace in
/// project guidance (e.g. a `OTTO.md` opening with a code fence
/// or indentation).
///
/// Ordering is fixed: default → override → body. LLMs weight later
/// instructions more heavily, so the project body wins on ambiguous
/// guidance.
pub fn layered_prompt(
    default: Option<&str>,
    override_prompt: Option<&str>,
    project_body: Option<&str>,
) -> Option<String> {
    let body_heading = format!("Project context (from {PROJECT_CONTEXT_FILE})");
    let layers: [(&str, Option<&str>); 3] = [
        ("Otto default prompt", default),
        ("Host override", override_prompt),
        (body_heading.as_str(), project_body),
    ];

    let mut sections: Vec<String> = Vec::new();
    for (heading, layer) in layers.iter() {
        if let Some(text) = layer {
            if !text.trim().is_empty() {
                // Render the original `text`, not the trimmed view —
                // leading/trailing whitespace may carry markdown
                // structure (fences, indentation) we must preserve.
                sections.push(format!("# {heading}\n\n{text}"));
            }
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_permissions_round_trip() {
        let parsed = parse_text(
            "---\n\
             permissions:\n  \
               allow:\n    \
                 - tool: run\n      \
                   command: cargo\n  \
               deny:\n    \
                 - tool: read_file\n      \
                   path: secret.txt\n\
             ---\n\
             body text\n",
        );
        assert_eq!(parsed.body.as_deref(), Some("body text"));
        assert_eq!(parsed.permissions.allow.len(), 1);
        assert_eq!(parsed.permissions.allow[0].tool, "run");
        assert_eq!(
            parsed.permissions.allow[0].command.as_deref(),
            Some("cargo")
        );
        assert_eq!(parsed.permissions.deny.len(), 1);
        assert_eq!(
            parsed.permissions.deny[0].path.as_deref(),
            Some("secret.txt")
        );
    }

    #[test]
    fn malformed_front_matter_falls_back_silently() {
        // Garbage YAML between the delimiters → permissions stays empty,
        // body still surfaces. The host should never refuse to start.
        let parsed = parse_text("---\n: : not yaml :\n---\nbody\n");
        assert_eq!(parsed.body.as_deref(), Some("body"));
        assert!(parsed.permissions.allow.is_empty());
        assert!(parsed.permissions.deny.is_empty());
    }

    #[test]
    fn no_front_matter_treats_whole_file_as_body() {
        let parsed = parse_text("just a body, no delimiters\n");
        assert_eq!(parsed.body.as_deref(), Some("just a body, no delimiters"));
        assert!(parsed.permissions.allow.is_empty());
    }

    #[test]
    fn front_matter_only_no_body() {
        let parsed = parse_text("---\npermissions:\n  allow: []\n---\n");
        assert!(parsed.body.is_none());
    }

    #[test]
    fn layered_all_three_renders_three_sections() {
        let s = layered_prompt(Some("DEFAULT"), Some("OVERRIDE"), Some("BODY")).unwrap();
        assert!(s.contains("# Otto default prompt\n\nDEFAULT"));
        assert!(s.contains("# Host override\n\nOVERRIDE"));
        assert!(s.contains("# Project context (from OTTO.md)\n\nBODY"));
        let i_default = s.find("DEFAULT").unwrap();
        let i_override = s.find("OVERRIDE").unwrap();
        let i_body = s.find("BODY").unwrap();
        assert!(i_default < i_override && i_override < i_body, "{}", s);
    }

    #[test]
    fn layered_default_only() {
        let s = layered_prompt(Some("D"), None, None).unwrap();
        assert!(s.starts_with("# Otto default prompt"));
        assert!(s.contains("D"));
        assert!(!s.contains("# Host override"));
        assert!(!s.contains("# Project context"));
    }

    #[test]
    fn layered_override_only() {
        let s = layered_prompt(None, Some("O"), None).unwrap();
        assert!(s.starts_with("# Host override"));
        assert!(s.contains("O"));
    }

    #[test]
    fn layered_body_only() {
        let s = layered_prompt(None, None, Some("B")).unwrap();
        assert!(s.starts_with("# Project context (from OTTO.md)"));
        assert!(s.contains("B"));
    }

    #[test]
    fn layered_none_returns_none() {
        assert!(layered_prompt(None, None, None).is_none());
    }

    #[test]
    fn layered_sections_use_h1_headings() {
        let s = layered_prompt(Some("a"), Some("b"), Some("c")).unwrap();
        for h in &[
            "# Otto default prompt",
            "# Host override",
            "# Project context (from OTTO.md)",
        ] {
            assert!(s.contains(h), "missing heading {h} in:\n{s}");
        }
    }

    #[test]
    fn layered_empty_string_layer_is_skipped() {
        // Empty strings collapse to absent — same as None.
        assert_eq!(
            layered_prompt(Some(""), Some(""), Some("")),
            layered_prompt(None, None, None),
        );
        let s = layered_prompt(Some(""), Some("O"), Some("")).unwrap();
        assert!(s.starts_with("# Host override"));
        assert!(!s.contains("# Otto default prompt"));
        assert!(!s.contains("# Project context"));
    }

    #[test]
    fn layered_whitespace_only_layer_is_skipped() {
        let s = layered_prompt(Some("   \n\t  "), Some("O"), Some("\n\n")).unwrap();
        assert!(s.starts_with("# Host override"));
        assert!(!s.contains("# Otto default prompt"));
        assert!(!s.contains("# Project context"));
    }

    #[test]
    fn layered_all_layers_whitespace_returns_none() {
        assert!(layered_prompt(Some("   "), Some("\n\n"), Some("\t")).is_none());
    }

    #[test]
    fn layered_preserves_code_fences_in_project_body() {
        // A OTTO.md that opens with a blank line then a code fence:
        // `trim()` would strip the leading newline; verbatim render
        // preserves it. This is the load-bearing test for the
        // verbatim-render rule when the surrounding whitespace itself
        // carries meaning.
        let body = "\n```rust\nfn main() {}\n```\n";
        let s = layered_prompt(None, None, Some(body)).unwrap();
        assert!(s.contains(body), "code fence altered:\n{s}");
    }

    #[test]
    fn layered_does_not_strip_leading_or_trailing_whitespace_of_non_empty_content() {
        // Inner whitespace must survive — trim() is only the emptiness gate.
        let s = layered_prompt(Some("  hello  "), None, None).unwrap();
        assert!(
            s.contains("  hello  "),
            "leading/trailing whitespace lost: {s}"
        );
    }

    #[test]
    fn parse_otto_md_pre_trims_body_before_layering() {
        // Pins the real parse → layer flow's whitespace behavior: even
        // though `layered_prompt` itself renders non-empty layers
        // verbatim, the body comes from `parse_text` which trims
        // leading/trailing whitespace at parse time. A `OTTO.md`
        // that opens with a blank line before a code fence loses that
        // newline before reaching the layered prompt. Document the
        // actual boundary here so callers know what to expect.
        let parsed = parse_text("\n```rust\nfn main() {}\n```\n");
        assert_eq!(parsed.body.as_deref(), Some("```rust\nfn main() {}\n```"));
        let s = layered_prompt(None, None, parsed.body.as_deref()).unwrap();
        assert!(
            s.contains("```rust\nfn main() {}\n```"),
            "code fence body should round-trip after parse + layer: {s}"
        );
    }
}
